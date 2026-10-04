use super::*;

fn apply(process: &mut Process, state: &Value, operations: Value) -> Value {
    let mut params = context(state);
    params["operations"] = operations;
    process.ok("objects.apply", params)
}

fn shape(id: &str, kind: &str, points: Value) -> Value {
    json!({"id": id, "kind": {"type": "shape", "shape": kind, "points": points,
        "style": {"color": {"r": 0, "g": 0, "b": 0, "a": 255}, "width": 2.0, "dashed": false}}})
}

fn connection(state: &Value, id: &str, line: &str, endpoint: usize, target: Value) -> Value {
    json!({"id": id, "page_id": state["page_id"], "line_id": line,
        "line_endpoint": endpoint, "target_id": "box", "target": target})
}

fn connect(process: &mut Process, state: &Value, connection: &Value) -> Value {
    let mut params = context(state);
    params["connection"] = connection.clone();
    process.ok("connections.connect", params)
}

fn objects(process: &mut Process, state: &Value) -> Value {
    process.ok("objects.list", context(state))["objects"].clone()
}

#[test]
fn connections_move_undo_persist_and_isolate_pages_and_documents() {
    let mut process = Process::headless();
    let initial = process.configure(false);
    let rectangle = shape(
        "box",
        "rectangle",
        json!([
            {"x": 0.0, "y": 0.0}, {"x": 100.0, "y": 0.0},
            {"x": 100.0, "y": 100.0}, {"x": 0.0, "y": 100.0}
        ]),
    );
    let line = shape(
        "line",
        "line",
        json!([{"x": -10.0, "y": -10.0}, {"x": 200.0, "y": 200.0}]),
    );
    let second = shape(
        "second",
        "line",
        json!([{"x": -20.0, "y": -20.0}, {"x": 300.0, "y": 300.0}]),
    );
    let state = apply(
        &mut process,
        &initial,
        json!([
            {"op": "add", "object": rectangle}, {"op": "add", "object": line}, {"op": "add", "object": second}
        ]),
    );
    let vertex = connection(
        &state,
        "vertex",
        "line",
        0,
        json!({"type": "vertex", "index": 0}),
    );
    let edge = connection(
        &state,
        "edge",
        "line",
        1,
        json!({"type": "edge", "start": 1, "end": 2, "t": 0.5}),
    );
    let shared = connection(
        &state,
        "shared",
        "second",
        0,
        json!({"type": "vertex", "index": 0}),
    );
    let state = connect(&mut process, &state, &vertex);
    let state = connect(&mut process, &state, &edge);
    let state = connect(&mut process, &state, &shared);
    assert_eq!(
        process.ok("connections.list", context(&state))["connections"],
        json!([vertex, edge, shared])
    );
    let before = objects(&mut process, &state);
    assert_eq!(
        before[1]["kind"]["points"],
        json!([{"x": 0.0, "y": 0.0}, {"x": 100.0, "y": 50.0}])
    );
    let mut moved = rectangle.clone();
    for point in moved["kind"]["points"].as_array_mut().unwrap() {
        point["x"] = json!(point["x"].as_f64().unwrap() + 20.0);
        point["y"] = json!(point["y"].as_f64().unwrap() + 30.0);
    }
    let state = apply(
        &mut process,
        &state,
        json!([{"op": "update", "object": moved}]),
    );
    let after = objects(&mut process, &state);
    assert_eq!(
        after[1]["kind"]["points"],
        json!([{"x": 20.0, "y": 30.0}, {"x": 120.0, "y": 80.0}])
    );
    assert_eq!(after[2]["kind"]["points"][0], json!({"x": 20.0, "y": 30.0}));
    let state = process.ok("undo", context(&state))["state"].clone();
    assert_eq!(objects(&mut process, &state), before);
    let state = process.ok("redo", context(&state))["state"].clone();
    assert_eq!(objects(&mut process, &state), after);
    let page_one = state["page_id"].clone();
    let page_two = process.ok("pages.add", context(&state))["state"].clone();
    assert_eq!(objects(&mut process, &page_two), json!([]));
    assert_eq!(
        process.ok("connections.list", context(&page_two))["connections"],
        json!([])
    );
    let mut cross_page = context(&page_two);
    let mut foreign = vertex.clone();
    foreign["page_id"] = page_two["page_id"].clone();
    cross_page["connection"] = foreign;
    error(
        process.call("connections.connect", cross_page),
        "invalid_document",
    );
    assert_eq!(process.ok("get_state", json!({})), page_two);
    let mut first = context(&page_two);
    first["page_id"] = page_one;
    assert_eq!(process.ok("objects.list", first.clone())["objects"], after);
    let selected = process.ok("pages.select", first.clone());
    assert_eq!(selected["revision"], page_two["revision"]);
    let mut disconnect = context(&selected);
    disconnect["connection_id"] = json!("edge");
    let state = process.ok("connections.disconnect", disconnect);
    assert_eq!(objects(&mut process, &state), after);
    assert_eq!(
        process.ok("connections.list", context(&state))["connections"],
        json!([vertex, shared])
    );
    let state = process.ok("undo", context(&state))["state"].clone();
    let state = apply(&mut process, &state, json!([{"op": "delete", "id": "box"}]));
    assert_eq!(
        process.ok("connections.list", context(&state))["connections"],
        json!([])
    );
    let state = process.ok("undo", context(&state))["state"].clone();
    assert_eq!(objects(&mut process, &state), after);
    assert_eq!(
        process.ok("connections.list", context(&state))["connections"],
        json!([vertex, edge, shared])
    );
    let path = process.directory.0.join("connections.neoboard");
    let saved = process.ok("document.save", json!({"path": path}));
    assert_eq!(saved["dirty"], false);
    let fresh = process.ok("document.new", json!({}));
    assert_ne!(fresh["document_id"], saved["document_id"]);
    error(
        process.call("connections.list", context(&saved)),
        "document_mismatch",
    );
    assert_eq!(
        process.ok("connections.list", context(&fresh))["connections"],
        json!([])
    );
    let reopened = process.ok("document.open", json!({"path": path}));
    assert_eq!(reopened["document_id"], saved["document_id"]);
    assert_eq!(objects(&mut process, &reopened), after);
    assert_eq!(
        process.ok("connections.list", context(&reopened))["connections"],
        json!([vertex, edge, shared])
    );
    let pages = process.ok(
        "pages.list",
        json!({"document_id": reopened["document_id"]}),
    );
    assert_eq!(pages["pages"].as_array().unwrap().len(), 2);
    process.close(false);
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

fn png() -> Vec<u8> {
    // 64x64 RGBA，单个无压缩 DEFLATE 块，跨越两个 8192 字节上传边界。
    let mut scanlines = Vec::new();
    for y in 0..64u8 {
        scanlines.push(0);
        for x in 0..64u8 {
            scanlines.extend_from_slice(&[x, y, 30, 255]);
        }
    }
    let length = u16::try_from(scanlines.len()).unwrap();
    let mut zlib = vec![0x78, 0x01, 0x01];
    zlib.extend_from_slice(&length.to_le_bytes());
    zlib.extend_from_slice(&(!length).to_le_bytes());
    zlib.extend_from_slice(&scanlines);
    let (mut a, mut b) = (1u32, 0u32);
    for byte in scanlines {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, data) in [
        (b"IHDR", vec![0, 0, 0, 64, 0, 0, 0, 64, 8, 6, 0, 0, 0]),
        (b"IDAT", zlib),
        (b"IEND", vec![]),
    ] {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = png.len();
        png.extend_from_slice(kind);
        png.extend_from_slice(&data);
        let crc = crc32(&png[start..]);
        png.extend_from_slice(&crc.to_be_bytes());
    }
    png
}

fn upload(process: &mut Process, bytes: &[u8]) -> Value {
    let begin = process.ok(
        "resources.begin",
        json!({"total_bytes": bytes.len(), "crc32": crc32(bytes)}),
    );
    assert_eq!(begin["max_chunk_bytes"], 8192);
    let mut offset = 0;
    for chunk in bytes.chunks(8192) {
        let result = process.ok(
            "resources.chunk",
            json!({"upload_id": begin["upload_id"], "offset": offset, "bytes": chunk}),
        );
        offset += chunk.len();
        assert_eq!(result["next_offset"], offset);
    }
    let asset = process.ok("resources.finish", json!({"upload_id": begin["upload_id"]}));
    assert_eq!(asset["mime_type"], "image/png");
    assert_eq!(asset["width"], 64);
    assert_eq!(asset["height"], 64);
    error(
        process.call("resources.finish", json!({"upload_id": begin["upload_id"]})),
        "upload_not_found",
    );
    asset
}

fn read_asset(process: &mut Process, asset: &Value, expected: &[u8]) {
    let mut bytes = Vec::new();
    while bytes.len() < expected.len() {
        let result = process.ok(
            "resources.read",
            json!({"asset_ref": asset, "offset": bytes.len(), "length": 8192}),
        );
        assert_eq!(result["asset_ref"], *asset);
        assert_eq!(result["mime_type"], "image/png");
        assert_eq!(result["offset"], bytes.len());
        assert_eq!(result["total_bytes"], expected.len());
        let chunk: Vec<u8> = serde_json::from_value(result["bytes"].clone()).unwrap();
        assert!(!chunk.is_empty() && chunk.len() <= 8192);
        bytes.extend_from_slice(&chunk);
        assert_eq!(result["next_offset"], bytes.len());
        assert_eq!(result["eof"], bytes.len() == expected.len());
    }
    assert_eq!(bytes, expected);
}

#[test]
fn chunked_png_upload_read_release_and_image_use() {
    let mut process = Process::headless();
    let initial = process.configure(false);
    let bytes = png();
    let asset = upload(&mut process, &bytes);
    read_asset(&mut process, &asset["asset_ref"], &bytes);
    assert_eq!(process.ok("get_state", json!({})), initial);
    assert_eq!(
        process.ok(
            "resources.release",
            json!({"asset_ref": asset["asset_ref"]})
        )["released"],
        true
    );
    error(
        process.call(
            "resources.read",
            json!({"asset_ref": asset["asset_ref"], "offset": 0, "length": 1}),
        ),
        "resource_not_found",
    );
    assert_eq!(
        process.ok(
            "resources.release",
            json!({"asset_ref": asset["asset_ref"]})
        )["released"],
        false
    );
    let asset = upload(&mut process, &bytes);
    let image = json!({"id": "image", "kind": {"type": "image", "position": {"x": 0.0, "y": 0.0},
        "width": 64.0, "height": 64.0, "asset_ref": asset["asset_ref"]}});
    let state = apply(
        &mut process,
        &initial,
        json!([{"op": "add", "object": image}]),
    );
    assert_eq!(state["revision"], 1);
    assert_eq!(objects(&mut process, &state), json!([image]));
    error(
        process.call(
            "resources.release",
            json!({"asset_ref": asset["asset_ref"]}),
        ),
        "resource_in_use",
    );
    process.close(true);
}

#[test]
fn oversized_frame_recovers_and_large_object_reads_as_utf8_bytes() {
    let mut process = Process::headless();
    let state = process.configure(false);
    let mut giant = object("large");
    giant["kind"]["text"] = json!("汉字𠀀".repeat(15000));
    let encoded = serde_json::to_vec(&giant).unwrap();
    assert!(encoded.len() > 65_536);
    let path = process.directory.0.join("large.neoboard");
    let fixture = json!({"version": 1, "document": {"id": state["document_id"],
        "pages": [{"id": state["page_id"], "objects": [giant, object("following")]}], "current_page": 0, "revision": 0, "connections": []}});
    std::fs::write(&path, serde_json::to_vec(&fixture).unwrap()).unwrap();
    let opened = process.ok("document.open", json!({"path": path}));
    let mut frame = vec![b'x'; 131_073];
    frame.extend_from_slice(b"\r\n");
    frame.extend_from_slice(b"{\"version\":1,\"type\":\"request\",\"id\":\"neo:after-oversize\",\"method\":\"get_state\",\"params\":{}}\n");
    process.raw(frame);
    assert_eq!(
        process.matching(|v| v["event"] == "protocol_error")["data"]["code"],
        "line_too_long"
    );
    assert_eq!(
        process.matching(|v| v["id"] == "neo:after-oversize")["result"],
        opened
    );
    let response = process.call("objects.list", context(&opened));
    error(response.clone(), "object_too_large");
    let data = &response["error"]["data"];
    assert_eq!(data["object_id"], "large");
    assert_eq!(data["next_offset"], 1);
    assert_eq!(data["total_bytes"], encoded.len());
    let mut bytes = Vec::new();
    let mut split_utf8 = false;
    while bytes.len() < encoded.len() {
        let mut params = context(&opened);
        params["object_id"] = json!("large");
        params["offset"] = json!(bytes.len());
        params["length"] = json!(8191);
        let result = process.ok("objects.read", params);
        assert_eq!(result["document_id"], opened["document_id"]);
        assert_eq!(result["page_id"], opened["page_id"]);
        assert_eq!(result["revision"], opened["revision"]);
        assert_eq!(result["object_id"], "large");
        assert_eq!(result["encoding"], "utf8_json_u8_array");
        assert_eq!(result["offset"], bytes.len());
        assert_eq!(result["total_bytes"], encoded.len());
        let chunk: Vec<u8> = serde_json::from_value(result["bytes"].clone()).unwrap();
        assert!(!chunk.is_empty() && chunk.len() <= 8191);
        split_utf8 |= std::str::from_utf8(&chunk).is_err();
        bytes.extend_from_slice(&chunk);
        assert_eq!(result["next_offset"], bytes.len());
        assert_eq!(result["eof"], bytes.len() == encoded.len());
    }
    assert!(split_utf8, "必须覆盖跨块 UTF-8 字符");
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), giant);
    let mut continuation = context(&opened);
    continuation["offset"] = data["next_offset"].clone();
    assert_eq!(
        process.ok("objects.list", continuation)["objects"],
        json!([object("following")])
    );
    let changed = add(&mut process, &opened, "revision-change");
    let mut stale = context(&opened);
    stale["object_id"] = json!("large");
    stale["offset"] = json!(0);
    stale["length"] = json!(8192);
    error(process.call("objects.read", stale), "revision_conflict");
    assert_eq!(changed["revision"], 1);
    process.close(true);
}

fn recovery(broken_stdout: bool) {
    let mut process = Process::headless();
    let initial = process.configure(false);
    let png = png();
    let asset = upload(&mut process, &png);
    let image = json!({"id": "recover-image", "kind": {"type": "image", "position": {"x": 12.0, "y": 34.0},
        "width": 64.0, "height": 64.0, "asset_ref": asset["asset_ref"]}});
    let state = apply(
        &mut process,
        &initial,
        json!([
            {"op": "add", "object": object("recover-text")}, {"op": "add", "object": image}
        ]),
    );
    assert_eq!(state["dirty"], true);
    let expected = objects(&mut process, &state);
    if broken_stdout {
        // 在一个无状态变化的屏障响应之后关闭唯一的 stdout 读端。
        process.break_output.store(true, Ordering::Release);
        process.ok("get_state", json!({}));
        process
            .output_closed
            .recv_timeout(TIMEOUT)
            .expect("stdout 读端未关闭");
        process.send(json!({"version": 1, "type": "request", "id": "neo:broken-output", "method": "get_state", "params": {}}));
        // 保留 stdin，确保输出错误本身终止进程，而不是依赖 EOF。
    } else {
        process.input.take();
    }
    let (status, _, errors) = process.finish();
    assert_eq!(
        status.success(),
        !broken_stdout,
        "{}",
        String::from_utf8_lossy(&errors)
    );
    let directory = process
        .directory
        .0
        .join("NeoRuntime-drawing")
        .join("recovery");
    let paths: Vec<_> = std::fs::read_dir(&directory)
        .expect("缺少恢复目录")
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(paths.len(), 1, "仅应保存一个恢复包");
    assert_eq!(paths[0].extension().unwrap(), "neoboard");
    assert!(
        paths[0]
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("recovery-")
    );
    let diagnostic = String::from_utf8(errors).unwrap();
    assert!(
        diagnostic.contains(paths[0].to_str().unwrap()),
        "stderr 未提供恢复路径：{diagnostic}"
    );
    let mut reopened = Process::headless();
    let fresh = reopened.configure(false);
    assert_ne!(
        fresh["document_id"], state["document_id"],
        "不得自动打开恢复包"
    );
    let restored = reopened.ok("document.open", json!({"path": paths[0]}));
    assert_eq!(restored["document_id"], state["document_id"]);
    assert_eq!(restored["page_id"], state["page_id"]);
    assert_eq!(restored["revision"], state["revision"]);
    assert_eq!(restored["dirty"], false);
    assert_eq!(objects(&mut reopened, &restored), expected);
    read_asset(&mut reopened, &asset["asset_ref"], &png);
    reopened.close(false);
}

#[test]
fn eof_dirty_recovery_is_isolated_and_explicitly_reopened() {
    recovery(false);
}

#[test]
fn broken_stdout_exits_with_stdin_open_and_recovers() {
    recovery(true);
}

#[test]
fn thousand_requests_correlate_ids_results_and_events() {
    let mut process = Process::headless();
    process.configure(false);
    let deadline = Instant::now() + Duration::from_secs(30);
    // 流水批次故意倒序关联响应，不能假定相邻帧或响应到达顺序。
    for batch in 0..50 {
        assert!(Instant::now() < deadline, "1000 请求正确性回归超时");
        for index in 0..20 {
            let number = batch * 20 + index;
            let (method, params) = match number % 4 {
                0 => (
                    "math.calculate",
                    json!({"expression": format!("{number}+7")}),
                ),
                1 => ("get_state", json!({})),
                2 => ("show", json!({})),
                _ => ("hide", json!({})),
            };
            process.send(json!({"version": 1, "type": "request", "id": format!("neo:loop-{number}"), "method": method, "params": params}));
        }
        for index in (0..20).rev() {
            let number = batch * 20 + index;
            let id = format!("neo:loop-{number}");
            let response = process.matching(|v| v["type"] == "response" && v["id"] == id);
            assert_eq!(response["ok"], true, "{response}");
            if number % 4 == 0 {
                assert_eq!(response["result"]["result"], (number + 7).to_string());
            } else {
                assert_eq!(response["result"]["revision"], 0);
                assert_eq!(response["result"]["dirty"], false);
                assert_eq!(response["result"]["visible"], false);
                if number % 4 >= 2 {
                    assert_eq!(response["result"]["desired_visible"], number % 4 == 2);
                }
            }
        }
        assert!(process.pending.iter().all(|v| v["type"] == "event"));
        process.pending.clear();
    }
    process.close(false);
}
