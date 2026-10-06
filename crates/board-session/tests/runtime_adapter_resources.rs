use board_protocol::{
    MAX_LINE_BYTES, Message, ProtocolError, Request, Response, read_message, write_message,
};
use board_session::{AppKind, Session};
use serde_json::{Value, json};
use std::io::Cursor;

const CHUNK: usize = 8192;

fn wire(message: &Message) -> Message {
    let mut bytes = Vec::new();
    write_message(&mut bytes, message).unwrap();
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert!(bytes.len() - 1 <= MAX_LINE_BYTES);
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["version"],
        1
    );
    read_message(&mut Cursor::new(bytes)).unwrap().unwrap()
}

fn deliver(session: &mut Session, frame: Value) -> Vec<Message> {
    let mut bytes = serde_json::to_vec(&frame).unwrap();
    assert!(bytes.len() <= MAX_LINE_BYTES, "oversized fixture request");
    bytes.push(b'\n');
    let out = match read_message(&mut Cursor::new(bytes)).unwrap().unwrap() {
        Message::Request(r) => session.handle(r),
        Message::Response(r) => session.handle_response(r),
        Message::Event(e) => session.handle_event(e),
    };
    out.iter().map(wire).collect()
}

fn call_as(session: &mut Session, owner: &str, method: &str, params: Value) -> Vec<Message> {
    let id = format!("{owner}:{}", board_core::new_id());
    let out = deliver(
        session,
        json!({"version":1,"type":"request","id":id,
        "method":method,"params":params}),
    );
    assert_eq!(response(&out).id, id);
    out
}

fn call(session: &mut Session, method: &str, params: Value) -> Vec<Message> {
    call_as(session, "neo", method, params)
}

fn response(out: &[Message]) -> &Response {
    let responses: Vec<_> = out
        .iter()
        .filter_map(|m| match m {
            Message::Response(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(responses.len(), 1, "{out:?}");
    responses[0]
}

fn result(out: &[Message]) -> Value {
    let r = response(out);
    assert!(r.ok, "{:?}", r.error);
    r.result.clone().unwrap()
}

fn failure(out: &[Message], code: &str) -> ProtocolError {
    let r = response(out);
    assert!(!r.ok);
    let error = r.error.clone().unwrap();
    assert_eq!(error.code, code);
    error
}

fn configured() -> Session {
    let mut session = Session::new(AppKind::Blackboard);
    let Message::Event(ready) = wire(&session.ready()) else {
        panic!("ready")
    };
    assert_eq!(ready.data["document_file_versions"], json!([1, 2, 3]));
    assert!(
        ready.data["object_types"]
            .as_array()
            .unwrap()
            .contains(&json!("handwritten"))
    );
    result(&call(
        &mut session,
        "configure",
        json!({"classroom_safe":false,
        "desktop_capture_allowed":true,"agent_allowed":false}),
    ));
    session
}

fn context(session: &mut Session) -> Value {
    let state = result(&call(session, "get_state", json!({})));
    json!({"document_id":state["document_id"],"page_id":state["page_id"],
        "expected_revision":state["revision"]})
}

fn apply(session: &mut Session, operations: Value) {
    let mut p = context(session);
    p["operations"] = operations;
    result(&call(session, "objects.apply", p));
}

fn handwritten(id: &str, points: usize, layout: bool) -> Value {
    json!({"id":id,"kind":{"type":"handwritten","position":{"x":80,"y":120},
        "text":"完整答案：五分之三 = 3/5；√二 🖊\n局部笔迹",
        "layout": if layout { json!({"type":"row","value":[
            {"type":"fraction","value":[{"type":"text","value":"三"},
                {"type":"text","value":"五"}]},
            {"type":"radical","value":{"type":"text","value":"二"}}]}) } else { Value::Null },
        "strokes":[{"points":(0..points).map(|i| json!({"x":i % 8,"y":i % 4,
            "time":0,"pressure":1})).collect::<Vec<_>>(),
            "style":{"color":{"r":32,"g":64,"b":128,"a":200},"width":2.5,"dashed":false}},
            {"points":[{"x":-2,"y":3,"time":0.25,"pressure":0.5}],
            "style":{"color":{"r":255,"g":0,"b":64,"a":255},"width":1.25,"dashed":true}}]}})
}

fn canonical(object: &Value) -> Vec<u8> {
    let object: board_core::BoardObject = serde_json::from_value(object.clone()).unwrap();
    serde_json::to_vec(&object).unwrap()
}

fn read_object(session: &mut Session, object: &Value) {
    let expected = canonical(object);
    let mut p = context(session);
    p["object_id"] = object["id"].clone();
    // Force the first boundary inside a Chinese UTF-8 code point.
    let split = expected.iter().position(|byte| *byte >= 0x80).unwrap() + 1;
    let mut bytes = Vec::new();
    loop {
        p["offset"] = json!(bytes.len());
        p["length"] = json!(if bytes.is_empty() { split } else { CHUNK });
        let r = result(&call(session, "objects.read", p.clone()));
        assert_eq!(r["encoding"], "utf8_json_u8_array");
        assert_eq!(r["document_id"], p["document_id"]);
        assert_eq!(r["page_id"], p["page_id"]);
        assert_eq!(r["revision"], p["expected_revision"]);
        assert_eq!(r["object_id"], object["id"]);
        assert_eq!(r["offset"], bytes.len());
        assert_eq!(r["total_bytes"], expected.len());
        let part: Vec<u8> = serde_json::from_value(r["bytes"].clone()).unwrap();
        assert!(!part.is_empty() && part.len() <= CHUNK);
        if bytes.is_empty() {
            assert!(std::str::from_utf8(&part).is_err());
        }
        bytes.extend(part);
        assert_eq!(r["next_offset"], bytes.len());
        assert_eq!(r["eof"], bytes.len() == expected.len());
        if r["eof"] == true {
            break;
        }
    }
    assert_eq!(bytes, expected);
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap(),
        serde_json::from_slice::<Value>(&expected).unwrap()
    );
    p["offset"] = json!(bytes.len());
    let eof = result(&call(session, "objects.read", p.clone()));
    assert_eq!(eof["bytes"], json!([]));
    assert_eq!(eof["eof"], true);
    for length in [0, CHUNK + 1] {
        p["length"] = json!(length);
        failure(&call(session, "objects.read", p.clone()), "invalid_params");
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn png(side: u32, mut seed: u32) -> Vec<u8> {
    let pixels: Vec<_> = (0..side * side * 4)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as u8
        })
        .collect();
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, side, side);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
    }
    bytes
}

fn begin(session: &mut Session, total: usize, crc: u32) -> Value {
    let r = result(&call(
        session,
        "resources.begin",
        json!({"total_bytes":total,"crc32":crc}),
    ));
    assert_eq!(r["max_chunk_bytes"], CHUNK);
    r["upload_id"].clone()
}

fn chunks(session: &mut Session, upload: &Value, bytes: &[u8]) {
    for (i, part) in bytes.chunks(CHUNK).enumerate() {
        let r = result(&call(
            session,
            "resources.chunk",
            json!({"upload_id":upload,
            "offset":i * CHUNK,"bytes":part}),
        ));
        assert_eq!(r["next_offset"], i * CHUNK + part.len());
    }
}

fn upload(session: &mut Session, bytes: &[u8]) -> Value {
    let id = begin(session, bytes.len(), crc32(bytes));
    chunks(session, &id, bytes);
    result(&call(session, "resources.finish", json!({"upload_id":id})))["asset_ref"].clone()
}

fn read_resource(session: &mut Session, asset: &Value, expected: &[u8]) {
    let mut bytes = Vec::new();
    loop {
        let r = result(&call(
            session,
            "resources.read",
            json!({"asset_ref":asset,
            "offset":bytes.len(),"length":CHUNK}),
        ));
        assert_eq!(r["asset_ref"], *asset);
        assert_eq!(r["mime_type"], "image/png");
        assert_eq!(r["offset"], bytes.len());
        assert_eq!(r["total_bytes"], expected.len());
        let part: Vec<u8> = serde_json::from_value(r["bytes"].clone()).unwrap();
        assert!(!part.is_empty() && part.len() <= CHUNK);
        bytes.extend(part);
        assert_eq!(r["next_offset"], bytes.len());
        assert_eq!(r["eof"], bytes.len() == expected.len());
        if r["eof"] == true {
            break;
        }
    }
    assert_eq!(bytes, expected);
}

fn reclaimed(session: &mut Session) {
    // Probe both byte reservations and upload slots through the adapter, not store internals.
    for (count, size) in [(4, 8 * 1024 * 1024), (16, 1)] {
        let ids: Vec<_> = (0..count).map(|_| begin(session, size, 0)).collect();
        failure(
            &call(
                session,
                "resources.begin",
                json!({"total_bytes":1,"crc32":0}),
            ),
            "resource_limit",
        );
        for id in ids {
            assert_eq!(
                result(&call(session, "resources.abort", json!({"upload_id":id})))["aborted"],
                true
            );
        }
    }
}

struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "runtime-adapter-resources-{}",
            board_core::new_id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn runtime_adapter_resources_handwritten_apply_list_read_preserves_complete_answer() {
    let mut session = configured();
    let mut objects = vec![
        handwritten("with-layout", 3, true),
        handwritten("without-layout", 2, false),
    ];
    apply(
        &mut session,
        json!(
            objects
                .iter()
                .map(|object| json!({"op":"add","object":object}))
                .collect::<Vec<_>>()
        ),
    );
    objects[0]["kind"]["position"] = json!({"x":320,"y":-40});
    apply(&mut session, json!([{"op":"update","object":objects[0]}]));
    let mut p = context(&mut session);
    p["limit"] = json!(1);
    for (index, object) in objects.iter().enumerate() {
        p["offset"] = json!(index);
        let r = result(&call(&mut session, "objects.list", p.clone()));
        assert_eq!(
            r["objects"],
            json!([serde_json::from_slice::<Value>(&canonical(object)).unwrap()])
        );
        assert_eq!(
            r["next_offset"],
            if index == 0 { json!(1) } else { Value::Null }
        );
        read_object(&mut session, object);
    }
}

#[test]
fn runtime_adapter_resources_larger_than_frame_handwritten_reassembles_exact_utf8() {
    let mut session = configured();
    // Compact integer input fits JSONL1; persisted floating-point fields expand beyond a frame.
    let object = handwritten("large-answer", 1600, true);
    assert!(canonical(&object).len() > MAX_LINE_BYTES);
    apply(&mut session, json!([{"op":"add","object":object}]));
    let mut p = context(&mut session);
    p["max_bytes"] = json!(MAX_LINE_BYTES);
    let error = failure(
        &call(&mut session, "objects.list", p.clone()),
        "object_too_large",
    );
    let data = error.data.unwrap();
    assert_eq!(data["object_id"], object["id"]);
    assert_eq!(data["document_id"], p["document_id"]);
    assert_eq!(data["page_id"], p["page_id"]);
    assert_eq!(data["revision"], p["expected_revision"]);
    assert_eq!(data["total_bytes"], canonical(&object).len());
    assert_eq!(data.get("next_offset"), Some(&Value::Null));
    read_object(&mut session, &object);
    p["offset"] = json!(1);
    let end = result(&call(&mut session, "objects.list", p));
    assert_eq!(end["objects"], json!([]));
    assert_eq!(end.get("next_offset"), Some(&Value::Null));
}

#[test]
fn runtime_adapter_resources_v3_package_roundtrips_adapter_created_ink_and_image() {
    let scratch = Scratch::new();
    let path = scratch.0.join("answer.neoboard");
    let mut session = configured();
    let bytes = png(1, 42);
    let asset = upload(&mut session, &bytes);
    let ink = handwritten("frozen-answer", 4, true);
    let image = json!({"id":"image","kind":{"type":"image","position":{"x":0,"y":0},
        "width":1,"height":1,"asset_ref":asset}});
    apply(
        &mut session,
        json!([{"op":"add","object":ink},{"op":"add","object":image}]),
    );
    let p = context(&mut session);
    let before = result(&call(&mut session, "objects.list", p));
    let saved = result(&call(&mut session, "document.save", json!({"path":path})));
    assert_eq!(saved["dirty"], false);
    let package: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(package["format"], "board-session-package");
    assert_eq!(package["version"], 3);
    assert_eq!(package["resources"].as_array().unwrap().len(), 1);
    let mut reopened = configured();
    let state = result(&call(&mut reopened, "document.open", json!({"path":path})));
    assert_eq!(state["dirty"], false);
    assert_eq!(state["can_undo"], false);
    assert_eq!(state["can_redo"], false);
    let p = context(&mut reopened);
    assert_eq!(result(&call(&mut reopened, "objects.list", p)), before);
    read_object(&mut reopened, &ink);
    read_resource(&mut reopened, &asset, &bytes);
}

#[test]
fn runtime_adapter_resources_upload_bounds_offsets_crc_and_failure_reclamation() {
    let mut session = configured();
    let bytes = png(128, 42);
    assert!(bytes.len() > MAX_LINE_BYTES);
    assert_eq!(crc32(b"123456789"), 0xcbf43926);
    let before = context(&mut session);
    let id = begin(&mut session, bytes.len(), crc32(&bytes));
    for invalid in [
        json!([]),
        json!(vec![0u8; CHUNK + 1]),
        json!("AA=="),
        json!([256]),
        json!([-1]),
    ] {
        failure(
            &call(
                &mut session,
                "resources.chunk",
                json!({"upload_id":id,
            "offset":0,"bytes":invalid}),
            ),
            "invalid_params",
        );
    }
    failure(
        &call(
            &mut session,
            "resources.chunk",
            json!({"upload_id":id,
        "offset":1,"bytes":[0]}),
        ),
        "invalid_params",
    );
    chunks(&mut session, &id, &bytes);
    let asset = result(&call(
        &mut session,
        "resources.finish",
        json!({"upload_id":id}),
    ))["asset_ref"]
        .clone();
    assert_eq!(context(&mut session), before);
    read_resource(&mut session, &asset, &bytes);
    for (offset, length) in [(0, 0), (0, CHUNK + 1), (bytes.len() + 1, 1)] {
        failure(
            &call(
                &mut session,
                "resources.read",
                json!({"asset_ref":asset,
            "offset":offset,"length":length}),
            ),
            "invalid_params",
        );
    }
    assert_eq!(
        result(&call(
            &mut session,
            "resources.release",
            json!({"asset_ref":asset})
        ))["released"],
        true
    );
    for incomplete in [false, true] {
        let id = begin(
            &mut session,
            bytes.len(),
            crc32(&bytes) ^ u32::from(!incomplete),
        );
        if !incomplete {
            chunks(&mut session, &id, &bytes);
        }
        failure(
            &call(&mut session, "resources.finish", json!({"upload_id":id})),
            "resource_integrity",
        );
        failure(
            &call(&mut session, "resources.abort", json!({"upload_id":id})),
            "upload_not_found",
        );
        reclaimed(&mut session);
    }
}

#[test]
fn runtime_adapter_resources_upload_owner_is_prefix_not_individual_request_id() {
    for (owner, other) in [("neo", "runtime"), ("runtime", "neo")] {
        let mut session = configured();
        let bytes = png(1, 17);
        let id = result(&call_as(
            &mut session,
            owner,
            "resources.begin",
            json!({"total_bytes":bytes.len(),"crc32":crc32(&bytes)}),
        ))["upload_id"]
            .clone();
        for method in ["resources.chunk", "resources.finish", "resources.abort"] {
            failure(
                &call_as(
                    &mut session,
                    other,
                    method,
                    json!({"upload_id":id,
                "offset":0,"bytes":bytes}),
                ),
                "upload_owner_mismatch",
            );
        }
        assert_eq!(
            result(&call_as(
                &mut session,
                owner,
                "resources.chunk",
                json!({"upload_id":id,"offset":0,"bytes":bytes})
            ))["next_offset"],
            bytes.len()
        );
        let asset = result(&call_as(
            &mut session,
            owner,
            "resources.finish",
            json!({"upload_id":id}),
        ))["asset_ref"]
            .clone();
        read_resource(&mut session, &asset, &bytes);
        let id = result(&call_as(
            &mut session,
            owner,
            "resources.begin",
            json!({"total_bytes":1,"crc32":0}),
        ))["upload_id"]
            .clone();
        assert_eq!(
            result(&call_as(
                &mut session,
                owner,
                "resources.abort",
                json!({"upload_id":id})
            ))["aborted"],
            true
        );
        failure(
            &call_as(
                &mut session,
                owner,
                "resources.finish",
                json!({"upload_id":id}),
            ),
            "upload_not_found",
        );
    }
}

fn request(out: &[Message], method: &str) -> Request {
    out.iter()
        .find_map(|m| match m {
            Message::Request(r) if r.method == method => Some(r.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {method}: {out:?}"))
}

fn reply(session: &mut Session, id: &str, result: Value) -> Vec<Message> {
    deliver(
        session,
        json!({"version":1,"type":"response","id":id,"ok":true,"result":result}),
    )
}

#[test]
fn runtime_adapter_resources_colliding_host_descriptor_downloads_instead_of_reusing_local() {
    for fault in ["none", "offset", "crc"] {
        let mut session = configured();
        let local = png(1, 7);
        let remote = png(128, 91);
        let asset = upload(&mut session, &local);
        // Session-only window acknowledgements; no GUI or capture service is started.
        session.attach_window();
        let window = session.pending_window_request().unwrap().clone();
        for m in session.acknowledge_window(&window.request_id, window.visible) {
            wire(&m);
        }
        let mut p = context(&mut session);
        p["user_authorized"] = json!(true);
        let job = result(&call(&mut session, "capture.request", p))["job_id"].clone();
        let window = session.pending_window_request().unwrap().clone();
        assert!(!window.visible);
        let out: Vec<_> = session
            .acknowledge_window(&window.request_id, false)
            .iter()
            .map(wire)
            .collect();
        let capture = request(&out, "host.capture_region");
        assert_eq!(capture.params["windows_hidden_confirmed"], true);
        let mut out = reply(
            &mut session,
            &capture.id,
            json!({"asset_ref":asset,
            "total_bytes":remote.len(),"crc32":crc32(&remote) ^ u32::from(fault == "crc")}),
        );
        assert_eq!(session.state()["hide_lease_count"], 0);
        let mut offset = 0;
        loop {
            let read = request(&out, "resources.read");
            assert_eq!(read.params["asset_ref"], asset);
            assert_eq!(read.params["offset"], offset);
            let length = read.params["length"].as_u64().unwrap() as usize;
            assert!((1..=CHUNK).contains(&length));
            let end = (offset + length).min(remote.len());
            out = reply(
                &mut session,
                &read.id,
                json!({"asset_ref":asset,
                "offset":offset + usize::from(fault == "offset"),"total_bytes":remote.len(),
                "bytes":&remote[offset..end],"next_offset":end,"eof":end == remote.len()}),
            );
            offset = end;
            if fault == "offset" || offset == remote.len() {
                break;
            }
            assert!(
                !out.iter()
                    .any(|m| matches!(m, Message::Event(e) if e.event == "job.finished"))
            );
        }
        let done = out
            .iter()
            .find_map(|m| match m {
                Message::Event(e) if e.event == "job.finished" => Some(&e.data),
                _ => None,
            })
            .expect("job.finished");
        assert_eq!(done["job_id"], job);
        assert_eq!(done["ok"], fault == "none");
        if fault == "none" {
            let imported = &done["result"]["asset_ref"];
            assert_ne!(*imported, asset);
            read_resource(&mut session, imported, &remote);
            result(&call(
                &mut session,
                "resources.release",
                json!({"asset_ref":imported}),
            ));
        } else {
            assert_eq!(
                done["error"]["code"],
                if fault == "crc" {
                    "resource_integrity"
                } else {
                    "invalid_host_response"
                }
            );
        }
        assert_eq!(
            request(&out, "resources.release").params["asset_ref"],
            asset
        );
        assert_eq!(session.state()["pending_jobs"], 0);
        read_resource(&mut session, &asset, &local);
        result(&call(
            &mut session,
            "resources.release",
            json!({"asset_ref":asset}),
        ));
        reclaimed(&mut session);
    }
}
