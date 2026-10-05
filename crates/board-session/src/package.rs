use crate::{ResourceStore, Result, core_error, error};
use board_core::{Document, ObjectKind, new_id};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

const MAX_FILE_BYTES: u64 = 80 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Asset {
    asset_ref: String,
    png_hex: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Package {
    format: String,
    version: u32,
    document: Document,
    resources: Vec<Asset>,
}

fn io_error(e: std::io::Error) -> board_protocol::ProtocolError {
    error("io_error", &e.to_string())
}

fn image_refs(document: &Document) -> HashSet<&str> {
    document
        .pages
        .iter()
        .flat_map(|p| &p.objects)
        .filter_map(|o| match &o.kind {
            ObjectKind::Image { asset_ref, .. } => Some(asset_ref.as_str()),
            _ => None,
        })
        .collect()
}

pub(crate) fn save(document: &Document, resources: &ResourceStore, path: &Path) -> Result<()> {
    document.validate().map_err(core_error)?;
    let refs = image_refs(document);
    let bytes = if refs.is_empty() {
        document.to_json().map_err(core_error)?.into_bytes()
    } else {
        let mut assets = Vec::new();
        let mut refs: Vec<_> = refs.into_iter().collect();
        refs.sort_unstable();
        for id in refs {
            let png = resources
                .png_bytes(id)
                .ok_or_else(|| error("resource_not_found", "无法保存缺失图片的文档"))?;
            let mut hex = String::with_capacity(png.len() * 2);
            const DIGITS: &[u8] = b"0123456789abcdef";
            for b in png {
                hex.push(DIGITS[(b >> 4) as usize] as char);
                hex.push(DIGITS[(b & 15) as usize] as char);
            }
            assets.push(Asset {
                asset_ref: id.into(),
                png_hex: hex,
            });
        }
        serde_json::to_vec(&Package {
            format: "board-session-package".into(),
            version: document.file_version(),
            document: document.clone(),
            resources: assets,
        })
        .map_err(|_| error("invalid_document", "文档序列化失败"))?
    };
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(error("resource_limit", "文档包超过 80 MiB"));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temporary = parent.join(format!(".board-session-{}.tmp", new_id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(io_error)?;
    let result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        // 不提前删除目标；失败时旧文件保持原样。
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(io_error)
}

pub(crate) fn load(path: &Path) -> Result<(Document, ResourceStore)> {
    let file = File::open(path).map_err(io_error)?;
    if file.metadata().map_err(io_error)?.len() > MAX_FILE_BYTES {
        return Err(error("resource_limit", "文档包超过 80 MiB"));
    }
    let mut source = String::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_string(&mut source)
        .map_err(io_error)?;
    if source.len() as u64 > MAX_FILE_BYTES {
        return Err(error("resource_limit", "文档包过大"));
    }
    // 先只读取顶层格式，资源始终是内嵌数据，绝不将资源引用作为文件路径。
    #[derive(Deserialize)]
    struct Header {
        format: Option<String>,
        version: Option<u64>,
    }
    let header: Header =
        serde_json::from_str(&source).map_err(|_| error("invalid_document", "无效文档 JSON"))?;
    let mut store = ResourceStore::default();
    let document = if let Some(format) = header.format {
        if format != "board-session-package" {
            return Err(error("invalid_document", "未知文档包格式"));
        }
        if !matches!(header.version, Some(1 | 2 | 3)) {
            return Err(error("unsupported_version", "不支持文档包版本"));
        }
        let package: Package =
            serde_json::from_str(&source).map_err(|_| error("invalid_document", "无效文档包"))?;
        if package.document.file_version() > package.version {
            return Err(error("invalid_document", "对象类型需要更高的文档包版本"));
        }
        package.document.validate().map_err(core_error)?;
        if package.resources.len() > 256 {
            return Err(error("resource_limit", "资源数超限"));
        }
        let refs = image_refs(&package.document);
        for asset in package.resources {
            // 包只保存当前文档资源；拒绝隐藏载荷及不受文档引用约束的资源。
            if !refs.contains(asset.asset_ref.as_str()) {
                return Err(error("invalid_resource", "文档包含未引用资源"));
            }
            if asset.png_hex.len() % 2 != 0
                || asset.png_hex.len() > 2 * crate::resources::MAX_PNG_BYTES
            {
                return Err(error("invalid_resource", "资源编码长度无效"));
            }
            let bytes: std::result::Result<Vec<u8>, _> = asset
                .png_hex
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    let a = (pair[0] as char).to_digit(16);
                    let b = (pair[1] as char).to_digit(16);
                    a.zip(b)
                        .map(|(a, b)| ((a << 4) | b) as u8)
                        .ok_or_else(|| error("invalid_resource", "无效 hex"))
                })
                .collect();
            store.import_named(asset.asset_ref, bytes?)?;
        }
        package.document
    } else {
        Document::from_json(&source).map_err(|error_value| match error_value {
            board_core::Error::UnsupportedVersion(_) => {
                error("unsupported_version", "不支持文档版本")
            }
            other => core_error(other),
        })?
    };
    for id in image_refs(&document) {
        if store.get(id).is_none() {
            return Err(error("resource_not_found", "文档包缺失引用的图片资源"));
        }
    }
    Ok((document, store))
}
