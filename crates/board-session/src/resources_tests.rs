use super::*;

fn chunk(bytes: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = bytes.len();
    bytes.extend_from_slice(kind);
    bytes.extend_from_slice(data);
    let crc = crc32(&bytes[start..]);
    bytes.extend_from_slice(&crc.to_be_bytes());
}

fn zlib(raw: &[u8]) -> Vec<u8> {
    let length = u16::try_from(raw.len()).unwrap();
    let mut bytes = vec![0x78, 0x01, 0x01];
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(&(!length).to_le_bytes());
    bytes.extend_from_slice(raw);
    let (mut a, mut b) = (1u32, 0u32);
    for byte in raw {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    bytes.extend_from_slice(&((b << 16) | a).to_be_bytes());
    bytes
}

fn fixture(header: &[u8], compressed: &[u8], extra: Option<&[u8; 4]>) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut bytes, b"IHDR", header);
    if let Some(kind) = extra {
        chunk(&mut bytes, kind, &[0; 8]);
    }
    // 连续 IDAT 可以在任意压缩字节处分开，包括 Adler 校验值。
    for byte in compressed {
        chunk(&mut bytes, b"IDAT", &[*byte]);
    }
    chunk(&mut bytes, b"IEND", &[]);
    bytes
}

#[test]
fn png_checks_actual_scanlines_adler_truncation_and_animation() {
    let header = [0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0];
    let compressed = zlib(&[0, 1, 2, 3, 255]);
    let valid = fixture(&header, &compressed, None);
    let mut store = ResourceStore::default();
    store.import_png(valid.clone()).unwrap();
    let mut invalid = vec![
        fixture(&header, &zlib(&[0, 1, 2, 3]), None),
        fixture(&header, &zlib(&[0, 1, 2, 3, 255, 0]), None),
        fixture(&header, &zlib(&[0; 10]), None),
    ];
    let mut bad_adler = compressed.clone();
    *bad_adler.last_mut().unwrap() ^= 1;
    invalid.push(fixture(&header, &bad_adler, None));
    for length in 0..compressed.len() {
        invalid.push(fixture(&header, &compressed[..length], None));
    }
    for length in 0..valid.len() {
        if length != 0 {
            invalid.push(valid[..length].to_vec());
        }
    }
    for bytes in invalid {
        assert_eq!(store.import_png(bytes).unwrap_err().code, "invalid_png");
        assert_eq!(store.entries.len(), 1);
        assert_eq!(store.bytes, valid.len());
    }
    for kind in [b"acTL", b"fcTL", b"fdAT"] {
        assert_eq!(
            store
                .import_png(fixture(&header, &compressed, Some(kind)))
                .unwrap_err()
                .code,
            "resource_limit"
        );
    }
}

#[test]
fn png_interlaced_and_packed_scanline_budgets_accept_valid_images() {
    for (width, height) in [(1u32, 1u32), (9, 9)] {
        for depth in [1u8, 8, 16] {
            let mut header = Vec::new();
            header.extend_from_slice(&width.to_be_bytes());
            header.extend_from_slice(&height.to_be_bytes());
            header.extend_from_slice(&[depth, 0, 0, 0, 1]);
            let mut raw = Vec::new();
            for (x, y, dx, dy) in [
                (0, 0, 8, 8),
                (4, 0, 8, 8),
                (0, 4, 4, 8),
                (2, 0, 4, 4),
                (0, 2, 2, 4),
                (1, 0, 2, 2),
                (0, 1, 1, 2),
            ] {
                let pixels = (x..width).step_by(dx).count();
                if pixels > 0 {
                    for _ in (y..height).step_by(dy) {
                        raw.extend(vec![0; 1 + (pixels * depth as usize).div_ceil(8)]);
                    }
                }
            }
            assert_eq!(
                validate_png(&fixture(&header, &zlib(&raw), None)).unwrap(),
                (width, height)
            );
        }
    }
}

#[test]
fn compressed_bomb_with_tiny_header_is_rejected_and_failed_finish_releases_capacity() {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 1024, 1024);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&vec![0; 4 * 1024 * 1024])
            .unwrap();
    }
    bytes[16..20].copy_from_slice(&1u32.to_be_bytes());
    bytes[20..24].copy_from_slice(&1u32.to_be_bytes());
    let crc = crc32(&bytes[12..29]);
    bytes[29..33].copy_from_slice(&crc.to_be_bytes());
    let mut store = ResourceStore::default();
    let id = store
        .begin_upload("neo", bytes.len(), crc32(&bytes))
        .unwrap();
    for (index, part) in bytes.chunks(MAX_READ_BYTES).enumerate() {
        store
            .upload_chunk("neo", &id, index * MAX_READ_BYTES, part)
            .unwrap();
    }
    assert_eq!(
        store.finish_upload("neo", &id).unwrap_err().code,
        "invalid_png"
    );
    assert!(store.entries.is_empty());
    assert!(store.uploads.is_empty());
    assert_eq!((store.bytes, store.reserved), (0, 0));
    for _ in 0..4 {
        store.begin_upload("neo", MAX_PNG_BYTES, 0).unwrap();
    }
    assert_eq!(
        store.begin_upload("neo", 1, 0).unwrap_err().code,
        "resource_limit"
    );
}

#[test]
fn entry_and_upload_count_limits_include_reservations() {
    let header = [0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0];
    let bytes = fixture(&header, &zlib(&[0; 5]), None);
    let mut store = ResourceStore::default();
    for _ in 0..240 {
        store.import_png(bytes.clone()).unwrap();
    }
    for _ in 0..16 {
        store
            .begin_upload("neo", bytes.len(), crc32(&bytes))
            .unwrap();
    }
    assert_eq!(
        store.begin_upload("neo", 1, 0).unwrap_err().code,
        "resource_limit"
    );
    assert_eq!(
        store.import_png(bytes.clone()).unwrap_err().code,
        "resource_limit"
    );
    let id = store.uploads.keys().next().unwrap().clone();
    store.upload_chunk("neo", &id, 0, &bytes).unwrap();
    store.finish_upload("neo", &id).unwrap();
    assert_eq!(store.entries.len(), 241);
    assert_eq!(store.uploads.len(), 15);
    assert_eq!(
        store.begin_upload("neo", 1, 0).unwrap_err().code,
        "resource_limit"
    );
    store.abort_uploads();
    store.import_png(bytes).unwrap();
}

#[test]
fn deterministic_resource_operations_keep_accounting_and_limits() {
    let header = [0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0];
    let bytes = fixture(&header, &zlib(&[0; 5]), None);
    let mut store = ResourceStore::default();
    let mut random = 0x953c_8721u32;
    for _ in 0..1000 {
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        let upload = store.uploads.keys().min().cloned();
        match random % 7 {
            0 => {
                let _ = store.import_png(bytes.clone());
            }
            1 => {
                let _ = store.begin_upload("neo", bytes.len(), crc32(&bytes));
            }
            2 => {
                if let Some(id) = upload {
                    let _ = store.upload_chunk("neo", &id, 0, &bytes);
                }
            }
            3 => {
                if let Some(id) = upload {
                    let _ = store.finish_upload("neo", &id);
                }
            }
            4 => {
                if let Some(id) = upload {
                    assert!(store.abort_upload("runtime", &id).is_err());
                }
            }
            5 => {
                if let Some(id) = store.entries.keys().min().cloned() {
                    store.release(&id);
                }
            }
            _ => store.abort_uploads(),
        }
        assert_eq!(
            store.bytes,
            store.entries.values().map(|r| r.bytes.len()).sum::<usize>()
        );
        assert_eq!(
            store.reserved,
            store.uploads.values().map(|u| u.total).sum::<usize>()
        );
        assert!(store.bytes + store.reserved <= MAX_STORE_BYTES);
        assert!(store.entries.len() + store.uploads.len() <= MAX_ENTRIES);
        assert!(store.uploads.len() <= 16);
    }
}
