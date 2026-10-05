use super::{HandwritingFont, ImageResources, RenderError, Result, export_rect};
use ab_glyph::{Font, FontArc, FontVec};
use std::collections::HashMap;
use std::io::Cursor;

pub const MAX_RESOURCE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_RESOURCE_PIXELS: u64 = 16_777_216;
pub const MAX_FONT_BYTES: usize = 64 * 1024 * 1024;
const MAX_RESOURCES: usize = 256;

pub(crate) struct ImageResource {
    pub(crate) pixmap: tiny_skia::Pixmap,
    pub(crate) png: Vec<u8>,
    texture: Option<egui::TextureHandle>,
}

/// 调用方持有的有界资源集。引用仅作为键；从不解释为路径或 URL。
/// 字体必须显式注入（例如调用方读取 C:\\Windows\\Fonts\\msyh.ttc，index=0）。
#[derive(Default)]
pub struct RenderResources {
    pub(crate) images: HashMap<String, ImageResource>,
    pub(crate) font: Option<FontArc>,
    handwriting_font: Option<HandwritingFont>,
    pixels: u64,
    bytes: usize,
}

impl RenderResources {
    pub fn new() -> Self {
        Self::default()
    }

    /// 支持 TTF 和 TTC；失败时保留原字体。不自动使用替代字体掩盖缺字。
    pub fn set_font(&mut self, bytes: Vec<u8>, face_index: u32) -> Result<()> {
        if bytes.len() > MAX_FONT_BYTES {
            return Err(RenderError::ResourceLimit("字体超过 64 MiB"));
        }
        let font = FontVec::try_from_vec_and_index(bytes, face_index)
            .map_err(|_| RenderError::InvalidFont)?;
        let font = FontArc::new(font);
        self.handwriting_font = Some(HandwritingFont::new(font.clone()));
        self.font = Some(font);
        Ok(())
    }

    pub fn clear_font(&mut self) {
        self.font = None;
        self.handwriting_font = None;
    }

    /// Shares the current font and its bounded unstyled glyph cache, never textures.
    /// Successful set_font starts a new cache generation (even for identical bytes);
    /// failed set_font preserves it. clear_font detaches it. Existing snapshots retain
    /// their old font/cache until dropped. This clone never locks the sampling cache.
    pub fn handwriting_font(&self) -> Option<HandwritingFont> {
        self.handwriting_font.clone()
    }

    /// 严格解码静态 PNG，去掉附加元数据并规范化为 RGBA；替换操作失败不影响旧资源。
    pub fn insert_png(&mut self, asset_ref: impl Into<String>, bytes: &[u8]) -> Result<()> {
        let key = asset_ref.into();
        if key.trim().is_empty() || key.len() > 4096 {
            return Err(RenderError::InvalidObject("无效图片引用".into()));
        }
        if bytes.len() > MAX_RESOURCE_BYTES {
            return Err(RenderError::ResourceLimit("PNG 输入超过 64 MiB"));
        }
        let previous = self.images.get(&key);
        if previous.is_none() && self.images.len() >= MAX_RESOURCES {
            return Err(RenderError::ResourceLimit("图片数量超过 256"));
        }
        validate_png_chunks(bytes)?;
        let mut options = png::DecodeOptions::default();
        options.set_ignore_checksums(false);
        options.set_skip_ancillary_crc_failures(false);
        // 元数据不会用于绘制；不解压文本和 ICC，避免元数据压缩炸弹。
        options.set_ignore_text_chunk(true);
        options.set_ignore_iccp_chunk(true);
        let mut decoder = png::Decoder::new_with_options(Cursor::new(bytes), options);
        decoder.set_limits(png::Limits {
            bytes: MAX_RESOURCE_BYTES,
        });
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(decode_error)?;
        let info = reader.info();
        export_rect(info.width, info.height)?;
        if info.animation_control.is_some() {
            return Err(RenderError::Decoding("不支持动画 PNG".into()));
        }
        let count = u64::from(info.width) * u64::from(info.height);
        let old_pixels = previous.map_or(0, |r| {
            u64::from(r.pixmap.width()) * u64::from(r.pixmap.height())
        });
        let total_pixels = self.pixels - old_pixels + count;
        if total_pixels > MAX_RESOURCE_PIXELS || reader.output_buffer_size() > MAX_RESOURCE_BYTES {
            return Err(RenderError::ResourceLimit("图片总解码像素预算超限"));
        }
        validate_inflated_size(bytes, reader.info())?;
        let mut decoded = vec![0; reader.output_buffer_size()];
        let output = reader.next_frame(&mut decoded).map_err(decode_error)?;
        reader.finish().map_err(decode_error)?;
        let channels = output.color_type.samples();
        let mut pixmap = tiny_skia::Pixmap::new(output.width, output.height)
            .ok_or(RenderError::ResourceLimit("图片像素分配失败"))?;
        for (source, destination) in decoded[..output.buffer_size()]
            .chunks_exact(channels)
            .zip(pixmap.pixels_mut())
        {
            let (r, g, b, a) = match output.color_type {
                png::ColorType::Grayscale => (source[0], source[0], source[0], 255),
                png::ColorType::GrayscaleAlpha => (source[0], source[0], source[0], source[1]),
                png::ColorType::Rgb => (source[0], source[1], source[2], 255),
                png::ColorType::Rgba => (source[0], source[1], source[2], source[3]),
                png::ColorType::Indexed => {
                    return Err(RenderError::Decoding("未展开索引颜色".into()));
                }
            };
            *destination = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
        }
        let png = pixmap
            .encode_png()
            .map_err(|e| RenderError::Encoding(e.to_string()))?;
        let total_bytes = self.bytes - previous.map_or(0, |r| r.png.len()) + png.len();
        if total_bytes > MAX_RESOURCE_BYTES {
            return Err(RenderError::ResourceLimit("图片总编码字节预算超限"));
        }
        self.images.insert(
            key,
            ImageResource {
                pixmap,
                png,
                texture: None,
            },
        );
        self.pixels = total_pixels;
        self.bytes = total_bytes;
        Ok(())
    }

    pub fn remove_image(&mut self, asset_ref: &str) -> bool {
        if let Some(image) = self.images.remove(asset_ref) {
            self.pixels -= u64::from(image.pixmap.width()) * u64::from(image.pixmap.height());
            self.bytes -= image.png.len();
            true
        } else {
            false
        }
    }

    /// 仅上传尚未上传的纹理。资源集应归属于一个 egui Context；换 Context 先 clear_textures。
    pub fn prepare_textures(&mut self, context: &egui::Context) {
        for image in self.images.values_mut() {
            if image.texture.is_none() {
                let pixels = image
                    .pixmap
                    .pixels()
                    .iter()
                    .map(|p| {
                        let p = p.demultiply();
                        egui::Color32::from_rgba_unmultiplied(
                            p.red(),
                            p.green(),
                            p.blue(),
                            p.alpha(),
                        )
                    })
                    .collect();
                let color = egui::ColorImage::new(
                    [
                        image.pixmap.width() as usize,
                        image.pixmap.height() as usize,
                    ],
                    pixels,
                );
                image.texture =
                    Some(context.load_texture("board-image", color, egui::TextureOptions::LINEAR));
            }
        }
    }

    pub fn clear_textures(&mut self) {
        for image in self.images.values_mut() {
            image.texture = None;
        }
    }

    pub(crate) fn check_text(&self, text: &str) -> Result<&FontArc> {
        let font = self.font.as_ref().ok_or(RenderError::MissingFont)?;
        for c in text.chars().filter(|c| !matches!(c, '\n' | '\r' | '\t')) {
            if font.glyph_id(c).0 == 0 {
                return Err(RenderError::MissingGlyph(c));
            }
        }
        Ok(font)
    }
}

impl ImageResources for RenderResources {
    fn texture_id(&self, asset_ref: &str) -> Option<egui::TextureId> {
        self.images
            .get(asset_ref)?
            .texture
            .as_ref()
            .map(egui::TextureHandle::id)
    }
}

fn validate_inflated_size(bytes: &[u8], info: &png::Info<'_>) -> Result<()> {
    let passes: &[(u32, u32, u32, u32)] = if info.interlaced {
        &[
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ]
    } else {
        &[(0, 0, 1, 1)]
    };
    let expected: usize = passes
        .iter()
        .map(|&(x, y, dx, dy)| {
            let width = info.width.saturating_sub(x).div_ceil(dx);
            let height = info.height.saturating_sub(y).div_ceil(dy);
            if width == 0 {
                0
            } else {
                info.raw_row_length_from_width(width) * height as usize
            }
        })
        .sum();
    let mut decoder = png::StreamingDecoder::new();
    decoder.set_ignore_adler32(false);
    decoder.set_skip_ancillary_crc_failures(false);
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    let mut offset = 0;
    let mut total = 0usize;
    let mut scratch = Vec::new();
    while offset < bytes.len() {
        // 小输入块限制单次 inflate 的膨胀；每次释放输出，避免积累恶意像素。
        let end = (offset + 256).min(bytes.len());
        let (used, event) = decoder
            .update(&bytes[offset..end], &mut scratch)
            .map_err(decode_error)?;
        offset += used;
        total = total.saturating_add(scratch.len());
        scratch.clear();
        if total > expected {
            return Err(RenderError::Decoding("解压像素超过 IHDR 声明大小".into()));
        }
        if matches!(event, png::Decoded::ImageEnd) {
            break;
        }
        if used == 0 && matches!(event, png::Decoded::Nothing) {
            return Err(RenderError::Decoding("PNG 解压未完成".into()));
        }
    }
    if total != expected {
        return Err(RenderError::Decoding("解压像素被截断".into()));
    }
    Ok(())
}

fn validate_png_chunks(bytes: &[u8]) -> Result<()> {
    let invalid = || RenderError::Decoding("PNG 截断、尾随数据或动画块".into());
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(invalid());
    }
    let mut offset = 8usize;
    while let Some(header) = bytes.get(offset..offset + 8) {
        let length = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let end = offset
            .checked_add(length)
            .and_then(|n| n.checked_add(12))
            .ok_or_else(invalid)?;
        if end > bytes.len() || matches!(&header[4..], b"acTL" | b"fcTL" | b"fdAT") {
            return Err(invalid());
        }
        if &header[4..] == b"IEND" {
            return if length == 0 && end == bytes.len() {
                Ok(())
            } else {
                Err(invalid())
            };
        }
        offset = end;
    }
    Err(invalid())
}

fn decode_error(error: png::DecodingError) -> RenderError {
    RenderError::Decoding(error.to_string())
}

pub(crate) fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        result.push(TABLE[(a >> 2) as usize] as char);
        result.push(TABLE[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        result.push(if chunk.len() > 1 {
            TABLE[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            TABLE[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    result
}
