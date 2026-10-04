use crate::{Result, error};
use board_core::new_id;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::Cursor;

pub const MAX_PNG_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_STORE_BYTES: usize = 32 * 1024 * 1024;
const MAX_DECODED_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_READ_BYTES: usize = 8192;
const MAX_ENTRIES: usize = 256;

#[derive(Debug, Clone)]
pub struct Resource {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
struct Upload {
    owner: String,
    total: usize,
    crc32: u32,
    bytes: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct ResourceStore {
    entries: HashMap<String, Resource>,
    bytes: usize,
    uploads: HashMap<String, Upload>,
    reserved: usize,
}

pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

pub(crate) fn valid_asset_ref(id: &str) -> bool {
    id.strip_prefix("asset:").is_some_and(|s| {
        !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

fn validate_png(bytes: &[u8]) -> Result<(u32, u32)> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(error("invalid_png", "无效 PNG 签名"));
    }
    // 显式验证每个块 CRC、IEND 和尾部，不能接受解码器忽略的尾随内容。
    let mut offset = 8;
    let mut ended = false;
    while offset < bytes.len() {
        if bytes.len() - offset < 12 {
            return Err(error("invalid_png", "PNG 块不完整"));
        }
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        if length > bytes.len() - offset - 12 {
            return Err(error("invalid_png", "PNG 块长度无效"));
        }
        let end = offset + 8 + length;
        if crc32(&bytes[offset + 4..end])
            != u32::from_be_bytes(bytes[end..end + 4].try_into().unwrap())
        {
            return Err(error("invalid_png", "PNG CRC 校验失败"));
        }
        let kind = &bytes[offset + 4..offset + 8];
        if kind == b"acTL" || kind == b"fcTL" || kind == b"fdAT" {
            return Err(error("resource_limit", "仅支持静态 PNG"));
        }
        offset = end + 4;
        if kind == b"IEND" {
            ended = length == 0 && offset == bytes.len();
            break;
        }
    }
    if !ended {
        return Err(error("invalid_png", "PNG 缺少 IEND 或包含尾随数据"));
    }
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_limits(png::Limits {
        bytes: MAX_DECODED_BYTES,
    });
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    decoder.ignore_checksums(false);
    let mut reader = decoder
        .read_info()
        .map_err(|_| error("invalid_png", "无效 PNG"))?;
    let width = reader.info().width;
    let height = reader.info().height;
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || u64::from(width) * u64::from(height) * 4 > MAX_DECODED_BYTES as u64
        || reader.output_buffer_size() > MAX_DECODED_BYTES
    {
        return Err(error("resource_limit", "PNG 解压尺寸超出限制"));
    }
    // png 0.17 的高层解码会丢弃多余扫描线，Limits 也不限制 IDAT 实际膨胀量。
    // 先逐块统计原始扫描线（包括过滤字节），超过 IHDR 的精确预算立即停止。
    let info = reader.info();
    let expected = if info.interlaced {
        [
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ]
        .into_iter()
        .map(|(x, y, dx, dy)| {
            let w = width.saturating_sub(x).div_ceil(dx);
            let h = height.saturating_sub(y).div_ceil(dy);
            if w == 0 {
                0
            } else {
                info.raw_row_length_from_width(w) * h as usize
            }
        })
        .sum()
    } else {
        info.raw_bytes()
    };
    let mut stream = png::StreamingDecoder::new();
    stream.set_ignore_adler32(false);
    stream.set_ignore_text_chunk(true);
    stream.set_ignore_iccp_chunk(true);
    let mut offset = 0;
    let mut total = 0usize;
    let mut block = Vec::new();
    let mut image_end = false;
    while offset < bytes.len() {
        block.clear();
        let (consumed, decoded) = stream
            .update(&bytes[offset..(offset + 8192).min(bytes.len())], &mut block)
            .map_err(|_| error("invalid_png", "PNG 压缩数据无效"))?;
        total += block.len();
        if total > expected {
            return Err(error("invalid_png", "PNG 实际解压长度超过声明尺寸"));
        }
        offset += consumed;
        if matches!(decoded, png::Decoded::ImageEnd) {
            image_end = true;
            break;
        }
        if consumed == 0 && block.is_empty() && matches!(decoded, png::Decoded::Nothing) {
            return Err(error("invalid_png", "PNG 压缩数据不完整"));
        }
    }
    if !image_end || total != expected {
        return Err(error("invalid_png", "PNG 实际解压长度与声明尺寸不匹配"));
    }
    let mut decoded = vec![0; reader.output_buffer_size()];
    reader
        .next_frame(&mut decoded)
        .map_err(|_| error("invalid_png", "PNG 解码失败"))?;
    reader
        .finish()
        .map_err(|_| error("invalid_png", "PNG 数据不完整"))?;
    Ok((width, height))
}

impl ResourceStore {
    fn capacity(&self, total: usize) -> Result<()> {
        if total == 0
            || total > MAX_PNG_BYTES
            || total > MAX_STORE_BYTES - self.bytes - self.reserved
            || self.entries.len() + self.uploads.len() >= MAX_ENTRIES
        {
            return Err(error(
                "resource_limit",
                "图片、资源数量或含上传预留的总内存超限",
            ));
        }
        Ok(())
    }

    pub fn import_png(&mut self, bytes: Vec<u8>) -> Result<String> {
        let id = format!("asset:{}", new_id());
        self.import_named(id.clone(), bytes)?;
        Ok(id)
    }

    pub(crate) fn import_named(&mut self, id: String, bytes: Vec<u8>) -> Result<()> {
        if !valid_asset_ref(&id) || self.entries.contains_key(&id) {
            return Err(error("invalid_resource", "资源引用无效或重复"));
        }
        self.capacity(bytes.len())?;
        let (width, height) = validate_png(&bytes)?;
        self.bytes += bytes.len();
        self.entries.insert(
            id,
            Resource {
                bytes,
                width,
                height,
            },
        );
        Ok(())
    }

    pub fn begin_upload(&mut self, owner: &str, total_bytes: usize, crc32: u32) -> Result<String> {
        if owner.is_empty() || owner.len() > 256 || self.uploads.len() >= 16 {
            return Err(error("resource_limit", "上传 owner 无效或并发上传超限"));
        }
        self.capacity(total_bytes)?;
        let id = format!("upload:{}", new_id());
        self.uploads.insert(
            id.clone(),
            Upload {
                owner: owner.into(),
                total: total_bytes,
                crc32,
                bytes: Vec::with_capacity(total_bytes),
            },
        );
        self.reserved += total_bytes;
        Ok(id)
    }

    fn upload(&self, owner: &str, id: &str) -> Result<&Upload> {
        let upload = self
            .uploads
            .get(id)
            .ok_or_else(|| error("upload_not_found", "上传不存在或已结束"))?;
        if upload.owner != owner {
            return Err(error("upload_owner_mismatch", "不能访问其他 owner 的上传"));
        }
        Ok(upload)
    }

    pub fn upload_chunk(
        &mut self,
        owner: &str,
        id: &str,
        offset: usize,
        bytes: &[u8],
    ) -> Result<usize> {
        let upload = self.upload(owner, id)?;
        if bytes.is_empty()
            || bytes.len() > MAX_READ_BYTES
            || offset != upload.bytes.len()
            || bytes.len() > upload.total - upload.bytes.len()
        {
            return Err(error(
                "invalid_params",
                "分块必须连续、非空且不超过 8192 字节和声明大小",
            ));
        }
        let upload = self.uploads.get_mut(id).unwrap();
        upload.bytes.extend_from_slice(bytes);
        Ok(upload.bytes.len())
    }

    // finish 无论校验成功与否均消费上传；未完成的上传也不能继续使用。
    pub fn finish_upload(&mut self, owner: &str, id: &str) -> Result<String> {
        self.upload(owner, id)?;
        let upload = self.uploads.remove(id).unwrap();
        self.reserved -= upload.total;
        if upload.bytes.len() != upload.total || crc32(&upload.bytes) != upload.crc32 {
            return Err(error("resource_integrity", "资源长度或 CRC32 不匹配"));
        }
        self.import_png(upload.bytes)
    }

    pub fn abort_upload(&mut self, owner: &str, id: &str) -> Result<()> {
        self.upload(owner, id)?;
        self.reserved -= self.uploads.remove(id).unwrap().total;
        Ok(())
    }

    pub fn abort_owner_uploads(&mut self, owner: &str) {
        let ids: Vec<_> = self
            .uploads
            .iter()
            .filter(|(_, u)| u.owner == owner)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            let _ = self.abort_upload(owner, &id);
        }
    }

    pub fn abort_uploads(&mut self) {
        self.uploads.clear();
        self.reserved = 0;
    }

    pub fn get(&self, asset_ref: &str) -> Option<&Resource> {
        self.entries.get(asset_ref)
    }

    pub fn png_bytes(&self, asset_ref: &str) -> Option<&[u8]> {
        self.get(asset_ref)
            .map(|resource| resource.bytes.as_slice())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Resource)> {
        self.entries
            .iter()
            .map(|(id, resource)| (id.as_str(), resource))
    }

    pub fn read(&self, asset_ref: &str, offset: usize, length: usize) -> Result<Value> {
        let resource = self
            .get(asset_ref)
            .ok_or_else(|| error("resource_not_found", "资源不存在"))?;
        if offset > resource.bytes.len() || length == 0 || length > MAX_READ_BYTES {
            return Err(error("invalid_params", "读取偏移无效或长度不在 1..8192 内"));
        }
        let end = (offset + length).min(resource.bytes.len());
        Ok(
            json!({"asset_ref": asset_ref, "mime_type": "image/png", "offset": offset,
            "total_bytes": resource.bytes.len(), "bytes": &resource.bytes[offset..end],
            "next_offset": end, "eof": end == resource.bytes.len()}),
        )
    }

    pub fn release(&mut self, asset_ref: &str) -> bool {
        if let Some(resource) = self.entries.remove(asset_ref) {
            self.bytes -= resource.bytes.len();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
#[path = "resources_tests.rs"]
mod tests;
