use board_protocol::{Message, Request, read_message, write_message};
use board_session::{AppKind, Session};
use serde_json::{Value, json};
use std::io::Cursor;

fn deliver(session: &mut Session, frame: Value) -> Vec<Message> {
    let mut bytes = serde_json::to_vec(&frame).unwrap();
    bytes.push(b'\n');
    let message = read_message(&mut Cursor::new(bytes)).unwrap().unwrap();
    let out = match message {
        Message::Request(request) => session.handle(request),
        Message::Response(response) => session.handle_response(response),
        Message::Event(event) => session.handle_event(event),
    };
    for message in &out {
        write_message(&mut Vec::new(), message).unwrap();
    }
    out
}

fn call(session: &mut Session, method: &str, params: Value) -> Vec<Message> {
    deliver(
        session,
        json!({"version": 1, "type": "request",
        "id": format!("neo:{}", board_core::new_id()), "method": method, "params": params}),
    )
}

fn reply(session: &mut Session, id: &str, result: Value) -> Vec<Message> {
    deliver(
        session,
        json!({"version": 1, "type": "response", "id": id,
        "ok": true, "result": result}),
    )
}

fn result(out: &[Message]) -> Value {
    out.iter()
        .find_map(|message| match message {
            Message::Response(response) => {
                assert!(response.ok, "{:?}", response.error);
                Some(response.result.clone().unwrap())
            }
            _ => None,
        })
        .expect("response")
}

fn request(out: &[Message], method: &str) -> Request {
    out.iter()
        .find_map(|message| match message {
            Message::Request(request) if request.method == method => Some(request.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {method}: {out:?}"))
}

fn finished(out: &[Message]) -> Value {
    out.iter()
        .find_map(|message| match message {
            Message::Event(event) if event.event == "job.finished" => Some(event.data.clone()),
            _ => None,
        })
        .expect("job.finished")
}

fn configured() -> Session {
    let mut session = Session::new(AppKind::Drawing);
    result(&call(
        &mut session,
        "configure",
        json!({"classroom_safe": false,
        "desktop_capture_allowed": true, "agent_allowed": true}),
    ));
    session
}

fn context(session: &Session) -> Value {
    let state = session.state();
    json!({"document_id": state["document_id"], "page_id": state["page_id"],
        "expected_revision": state["revision"]})
}

fn capture(session: &mut Session) -> (String, Request) {
    session.attach_window();
    let window = session.pending_window_request().unwrap().clone();
    session.acknowledge_window(&window.request_id, window.visible);
    let mut params = context(session);
    params["user_authorized"] = json!(true);
    let id = result(&call(session, "capture.request", params))["job_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let window = session.pending_window_request().unwrap().clone();
    assert!(!window.visible);
    let out = session.acknowledge_window(&window.request_id, false);
    (id, request(&out, "host.capture_region"))
}

fn png(pixel: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixel)
            .unwrap();
    }
    bytes
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

fn import(session: &mut Session, bytes: &[u8]) -> String {
    result(&call(
        session,
        "resources.import_png",
        json!({"bytes": bytes}),
    ))["asset_ref"]
        .as_str()
        .unwrap()
        .to_owned()
}

struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("board-session-regression-{}", board_core::new_id()));
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
fn release_regression_cancelled_async_capture_ignores_second_original_response() {
    for pending_before_cancel in [true, false] {
        for second in [
            json!({"asset_ref": "asset:stale"}),
            json!({"job_id": "host:other"}),
        ] {
            let mut session = configured();
            let (job, capture) = capture(&mut session);
            if pending_before_cancel {
                reply(&mut session, &capture.id, json!({"job_id": "host:active"}));
            }
            let first_cancel = request(
                &call(&mut session, "jobs.cancel", json!({"job_id": job})),
                "jobs.cancel",
            );
            let cancel = if pending_before_cancel {
                first_cancel
            } else {
                request(
                    &reply(&mut session, &capture.id, json!({"job_id": "host:active"})),
                    "jobs.cancel",
                )
            };
            let before = session.state();
            assert!(
                deliver(
                    &mut session,
                    json!({"version": 1, "type": "response",
                "id": capture.id, "ok": true, "result": second})
                )
                .is_empty(),
                "a second original response must not stop or rebind an async capture"
            );
            assert_eq!(session.state(), before);
            assert_eq!(before["hide_lease_count"], 1);
            assert!(!session.effective_visible());
            reply(
                &mut session,
                &cancel.id,
                json!({"cancelled": true, "job_id": "host:active"}),
            );
            assert_eq!(session.state()["hide_lease_count"], 0);
            assert!(session.effective_visible());
        }
    }
}

#[test]
fn release_regression_colliding_descriptor_downloads_and_persists_host_bytes() {
    let scratch = Scratch::new();
    let mut session = configured();
    let local = png([255, 0, 0, 255]);
    let remote = png([0, 0, 255, 255]);
    let asset = import(&mut session, &local);
    let (job, capture) = capture(&mut session);
    let out = reply(
        &mut session,
        &capture.id,
        json!({"asset_ref": asset,
        "total_bytes": remote.len(), "crc32": crc32(&remote)}),
    );
    let read = request(&out, "resources.read");
    assert_eq!(session.state()["hide_lease_count"], 0);
    assert_eq!(session.state()["pending_jobs"], 1);
    let out = reply(
        &mut session,
        &read.id,
        json!({"asset_ref": asset, "offset": 0,
        "total_bytes": remote.len(), "bytes": remote, "next_offset": remote.len(), "eof": true}),
    );
    let done = finished(&out);
    assert_eq!(done["job_id"], job);
    assert_eq!(done["ok"], true);
    let imported = done["result"]["asset_ref"].as_str().unwrap();
    assert_ne!(imported, asset);
    assert_eq!(
        session.resources.png_bytes(imported),
        Some(remote.as_slice())
    );
    assert_eq!(session.resources.png_bytes(&asset), Some(local.as_slice()));
    assert_eq!(
        request(&out, "resources.release").params["asset_ref"],
        asset
    );

    let mut params = context(&session);
    params["operations"] = json!([{"op": "add", "object": {"id": "captured-image",
        "kind": {"type": "image", "position": {"x": 0, "y": 0}, "width": 1,
        "height": 1, "asset_ref": imported}}}]);
    result(&call(&mut session, "objects.apply", params));
    let path = scratch.0.join("capture.neoboard");
    result(&call(&mut session, "document.save", json!({"path": path})));
    let mut reopened = configured();
    result(&call(&mut reopened, "document.open", json!({"path": path})));
    assert_eq!(
        reopened.resources.png_bytes(imported),
        Some(remote.as_slice())
    );
    assert!(reopened.resources.get(&asset).is_none());
    assert_eq!(reopened.state()["dirty"], false);
}

#[test]
fn release_regression_colliding_descriptor_checks_crc_and_reclaims_reservation() {
    let mut session = configured();
    let bytes = png([0, 255, 0, 255]);
    let asset = import(&mut session, &bytes);
    let (_, capture) = capture(&mut session);
    let read = request(
        &reply(
            &mut session,
            &capture.id,
            json!({"asset_ref": asset,
        "total_bytes": bytes.len(), "crc32": crc32(&bytes) ^ 1}),
        ),
        "resources.read",
    );
    let out = reply(
        &mut session,
        &read.id,
        json!({"asset_ref": asset, "offset": 0,
        "total_bytes": bytes.len(), "bytes": bytes, "next_offset": bytes.len(), "eof": true}),
    );
    assert_eq!(finished(&out)["error"]["code"], "resource_integrity");
    assert_eq!(
        request(&out, "resources.release").params["asset_ref"],
        asset
    );
    assert_eq!(session.resources.iter().count(), 1);
    assert_eq!(session.state()["pending_jobs"], 0);
    for _ in 0..16 {
        result(&call(
            &mut session,
            "resources.begin",
            json!({"total_bytes": 1, "crc32": 0}),
        ));
    }
}

#[test]
fn release_regression_colliding_descriptor_respects_upload_quota() {
    let mut session = configured();
    let bytes = png([1, 2, 3, 255]);
    let asset = import(&mut session, &bytes);
    for _ in 0..16 {
        result(&call(
            &mut session,
            "resources.begin",
            json!({"total_bytes": 1, "crc32": 0}),
        ));
    }
    let (_, capture) = capture(&mut session);
    let out = reply(
        &mut session,
        &capture.id,
        json!({"asset_ref": asset,
        "total_bytes": bytes.len(), "crc32": crc32(&bytes)}),
    );
    assert_eq!(finished(&out)["error"]["code"], "resource_limit");
    assert_eq!(
        request(&out, "resources.release").params["asset_ref"],
        asset
    );
    assert_eq!(session.resources.png_bytes(&asset), Some(bytes.as_slice()));
    assert_eq!(session.state()["hide_lease_count"], 0);
}

#[test]
fn release_regression_cancelled_colliding_descriptor_releases_only_host_copy() {
    for via_event in [false, true] {
        let mut session = configured();
        let bytes = png([1, 2, 3, 255]);
        let asset = import(&mut session, &bytes);
        let (job, capture) = capture(&mut session);
        call(&mut session, "jobs.cancel", json!({"job_id": job}));
        let descriptor = json!({"asset_ref": asset,
            "total_bytes": bytes.len(), "crc32": crc32(&bytes)});
        let out = if via_event {
            deliver(
                &mut session,
                json!({"version": 1, "type": "event", "event": "job.finished",
                "data": {"job_id": job, "request_id": capture.id, "ok": true, "result": descriptor}}),
            )
        } else {
            reply(&mut session, &capture.id, descriptor)
        };
        assert_eq!(
            request(&out, "resources.release").params["asset_ref"],
            asset
        );
        assert_eq!(session.resources.png_bytes(&asset), Some(bytes.as_slice()));
        assert_eq!(session.state()["hide_lease_count"], 0);
        assert_eq!(session.state()["pending_capture_cancellations"], 0);
    }
}

#[test]
fn release_regression_stale_colliding_descriptor_releases_only_host_copy() {
    let mut session = configured();
    let bytes = png([1, 2, 3, 255]);
    let asset = import(&mut session, &bytes);
    let (_, capture) = capture(&mut session);
    let params = context(&session);
    result(&call(&mut session, "pages.add", params));
    let out = reply(
        &mut session,
        &capture.id,
        json!({"asset_ref": asset,
        "total_bytes": bytes.len(), "crc32": crc32(&bytes)}),
    );
    assert_eq!(finished(&out)["error"]["code"], "revision_conflict");
    assert_eq!(
        request(&out, "resources.release").params["asset_ref"],
        asset
    );
    assert_eq!(session.resources.png_bytes(&asset), Some(bytes.as_slice()));
    assert_eq!(session.state()["hide_lease_count"], 0);
    assert_eq!(session.state()["pending_jobs"], 0);
}

#[test]
fn release_regression_invalid_discard_flag_is_rejected_before_mutation() {
    let scratch = Scratch::new();
    let path = scratch.0.join("source.neoboard");
    let mut source = configured();
    result(&call(&mut source, "document.save", json!({"path": path})));
    for method in ["document.new", "document.open", "close"] {
        for flag in [Value::Null, json!("true"), json!(0), json!([]), json!({})] {
            let mut session = configured();
            let before = session.state();
            let out = call(
                &mut session,
                method,
                json!({"discard_unsaved": flag, "path": path}),
            );
            let response = out
                .iter()
                .find_map(|message| match message {
                    Message::Response(response) => Some(response),
                    _ => None,
                })
                .unwrap();
            assert!(!response.ok, "{method} accepted a non-boolean discard flag");
            assert_eq!(response.error.as_ref().unwrap().code, "invalid_params");
            assert_eq!(session.state(), before);
        }
    }
}
