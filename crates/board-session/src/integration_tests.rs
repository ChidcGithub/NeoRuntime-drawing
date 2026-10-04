use super::*;
use board_core::{BoardObject, Point};

fn call(s: &mut Session, method: &str, p: Value) -> Vec<Message> {
    let out = s.handle(Request::new("neo:test", method, p).unwrap());
    check(&out);
    out
}
fn check(out: &[Message]) {
    for message in out {
        board_protocol::write_message(&mut Vec::new(), message).unwrap();
    }
}
fn result(out: &[Message]) -> Value {
    out.iter()
        .find_map(|m| match m {
            Message::Response(r) => {
                assert!(r.ok, "{:?}", r.error);
                Some(r.result.clone().unwrap())
            }
            _ => None,
        })
        .unwrap()
}
fn done(out: &[Message]) -> Value {
    check(out);
    out.iter()
        .find_map(|m| match m {
            Message::Event(e) if e.event == "job.finished" => Some(e.data.clone()),
            _ => None,
        })
        .unwrap()
}
fn request(out: &[Message], method: &str) -> Request {
    out.iter()
        .find_map(|m| match m {
            Message::Request(r) if r.method == method => Some(r.clone()),
            _ => None,
        })
        .unwrap()
}
fn session() -> Session {
    let mut s = Session::new(AppKind::Drawing);
    result(&call(
        &mut s,
        "configure",
        json!({"classroom_safe": false, "desktop_capture_allowed": true, "agent_allowed": true}),
    ));
    s
}
fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 128, 128);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut random = 42u32;
        let data: Vec<_> = (0..128 * 128 * 4)
            .map(|_| {
                random ^= random << 13;
                random ^= random >> 17;
                random ^= random << 5;
                random as u8
            })
            .collect();
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&data)
            .unwrap();
    }
    bytes
}
fn context(s: &Session) -> Value {
    json!({"document_id": s.document.id, "page_id": s.document.current_page().id, "expected_revision": s.document.revision})
}
fn ack(s: &mut Session) -> Vec<Message> {
    let r = s.pending_window_request().unwrap().clone();
    s.acknowledge_window(&r.request_id, r.visible)
}
fn capture(s: &mut Session) -> (String, Request) {
    s.attach_window();
    if s.pending_window_request().is_some() {
        ack(s);
    }
    let mut p = context(s);
    p["user_authorized"] = json!(true);
    let id = result(&call(s, "capture.request", p))["job_id"]
        .as_str()
        .unwrap()
        .to_owned();
    (id, request(&ack(s), "host.capture_region"))
}
fn image(s: &mut Session, id: &str, asset: &str) {
    let mut p = context(s);
    p["operations"] = json!([Operation::Add {
        object: BoardObject {
            id: id.into(),
            kind: ObjectKind::Image {
                position: Point::default(),
                width: 128.0,
                height: 128.0,
                asset_ref: asset.into()
            }
        }
    }]);
    result(&call(s, "objects.apply", p));
}

#[test]
fn chunk_upload_exceeds_frame_and_checks_owner_integrity_and_abort() {
    let bytes = png();
    assert!(bytes.len() > MAX_LINE_BYTES);
    let mut s = session();
    let id = result(&call(
        &mut s,
        "resources.begin",
        json!({"total_bytes": bytes.len(), "crc32": resources::crc32(&bytes)}),
    ))["upload_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let wrong = s.handle(
        Request::new("runtime:other", "resources.abort", json!({"upload_id": id})).unwrap(),
    );
    assert!(
        matches!(&wrong[0], Message::Response(r) if r.error.as_ref().unwrap().code == "upload_owner_mismatch")
    );
    for (i, chunk) in bytes.chunks(8192).enumerate() {
        assert_eq!(
            result(&call(
                &mut s,
                "resources.chunk",
                json!({"upload_id": id, "offset": i * 8192, "bytes": chunk})
            ))["next_offset"],
            (i * 8192 + chunk.len()) as u64
        );
    }
    let asset = result(&call(&mut s, "resources.finish", json!({"upload_id": id})))["asset_ref"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(s.resources.png_bytes(&asset), Some(bytes.as_slice()));
    assert_eq!(s.resources.iter().count(), 1);
    let id = s.resources.begin_upload("a", bytes.len(), 0).unwrap();
    for (i, chunk) in bytes.chunks(8192).enumerate() {
        s.resources.upload_chunk("a", &id, i * 8192, chunk).unwrap();
    }
    assert_eq!(
        s.resources.finish_upload("a", &id).unwrap_err().code,
        "resource_integrity"
    );
    assert_eq!(
        s.resources.abort_upload("a", &id).unwrap_err().code,
        "upload_not_found"
    );
    let id = s.resources.begin_upload("a", 100, 0).unwrap();
    assert!(s.resources.upload_chunk("a", &id, 1, &[0]).is_err());
    s.resources.abort_upload("a", &id).unwrap();
    assert!(s.resources.upload_chunk("a", &id, 0, &[0]).is_err());
}

#[test]
fn upload_reservations_and_png_bombs_crc_and_trailing_data_are_bounded() {
    let mut store = ResourceStore::default();
    let ids: Vec<_> = (0..4)
        .map(|_| {
            store
                .begin_upload("a", resources::MAX_PNG_BYTES, 0)
                .unwrap()
        })
        .collect();
    assert_eq!(
        store.begin_upload("a", 1, 0).unwrap_err().code,
        "resource_limit"
    );
    assert_eq!(store.import_png(png()).unwrap_err().code, "resource_limit");
    store.abort_upload("a", &ids[0]).unwrap();
    store.import_png(png()).unwrap();
    store.abort_uploads();
    let mut bytes = png();
    bytes.push(0);
    assert_eq!(store.import_png(bytes).unwrap_err().code, "invalid_png");
    let mut bytes = png();
    bytes[32] ^= 1;
    assert_eq!(store.import_png(bytes).unwrap_err().code, "invalid_png");
    let mut bytes = png();
    bytes[16..20].copy_from_slice(&8192u32.to_be_bytes());
    bytes[20..24].copy_from_slice(&8192u32.to_be_bytes());
    let crc = resources::crc32(&bytes[12..29]);
    bytes[29..33].copy_from_slice(&crc.to_be_bytes());
    assert_eq!(store.import_png(bytes).unwrap_err().code, "resource_limit");
}

#[test]
fn package_all_pages_atomic_overwrite_missing_resources_and_history_policy() {
    let dir = std::env::temp_dir().join(format!("session-package-{}", new_id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("board.json");
    let mut s = session();
    let bytes = png();
    let a = s.resources.import_png(bytes.clone()).unwrap();
    image(&mut s, "first", &a);
    let p = context(&s);
    result(&call(&mut s, "pages.add", p));
    image(&mut s, "second", &a);
    s.save_document(&path).unwrap();
    assert!(!s.history.is_dirty(&s.document));
    let mut reopened = session();
    reopened.open_document(&path, false).unwrap();
    assert_eq!(reopened.document, s.document);
    assert_eq!(reopened.resources.png_bytes(&a), Some(bytes.as_slice()));
    assert!(!reopened.history.can_undo());
    assert!(s.history.can_undo());
    let p = context(&s);
    result(&call(&mut s, "undo", p));
    s.save_document(&path).unwrap();
    assert!(s.history.can_redo());
    let p = context(&s);
    result(&call(&mut s, "redo", p));
    assert!(s.resources.get(&a).is_some());
    let previous = std::fs::read(&path).unwrap();
    s.resources.release(&a);
    assert_eq!(
        s.save_document(&path).unwrap_err().code,
        "resource_not_found"
    );
    assert_eq!(std::fs::read(&path).unwrap(), previous);
    assert!(s.history.is_dirty(&s.document));
    let old = reopened.document.clone();
    let mut value: Value = serde_json::from_slice(&previous).unwrap();
    value["resources"][0]["asset_ref"] = json!("asset:../../escape");
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(reopened.open_document(&path, true).is_err());
    assert_eq!(reopened.document, old);
    value["resources"] = json!([]);
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(
        reopened.open_document(&path, true).unwrap_err().code,
        "resource_not_found"
    );
    assert_eq!(reopened.document, old);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn async_capture_downloads_verified_resource_and_releases_host_reference() {
    let mut s = session();
    let (job, r) = capture(&mut s);
    let out = s.handle_response(Response::success(&r.id, json!({"job_id": "host:1"})).unwrap());
    assert!(
        !out.iter()
            .any(|m| matches!(m, Message::Event(e) if e.event == "job.finished"))
    );
    assert!(!s.effective_visible());
    let bytes = png();
    let mut out = s.handle_event(Event::new("jobs.finished", json!({"job_id": "host:1", "request_id": r.id, "ok": true,
        "result": {"asset_ref": "asset:remote", "total_bytes": bytes.len(), "crc32": resources::crc32(&bytes)}})));
    assert!(s.effective_visible());
    loop {
        check(&out);
        let r = request(&out, "resources.read");
        let offset = r.params["offset"].as_u64().unwrap() as usize;
        let end = (offset + r.params["length"].as_u64().unwrap() as usize).min(bytes.len());
        out = s.handle_response(Response::success(r.id, json!({"asset_ref": "asset:remote", "offset": offset,
            "total_bytes": bytes.len(), "bytes": &bytes[offset..end], "next_offset": end, "eof": end == bytes.len()})).unwrap());
        if end == bytes.len() {
            break;
        }
    }
    let finished = done(&out);
    assert_eq!(finished["ok"], true);
    assert_eq!(finished["job_id"], job);
    let asset = finished["result"]["asset_ref"].as_str().unwrap();
    assert_eq!(s.resources.png_bytes(asset), Some(bytes.as_slice()));
    assert_eq!(
        request(&out, "resources.release").params["asset_ref"],
        "asset:remote"
    );
    assert!(
        s.handle_event(Event::new(
            "job.finished",
            json!({"job_id": "host:1", "ok": true, "result": {}})
        ))
        .is_empty()
    );
}

#[test]
fn cancelled_capture_acknowledgement_is_not_completion_and_late_event_releases_lease() {
    let mut s = session();
    let (id, r) = capture(&mut s);
    call(&mut s, "jobs.cancel", json!({"job_id": id}));
    let out = s.handle_response(Response::success(&r.id, json!({"job_id": "host:late"})).unwrap());
    assert_eq!(request(&out, "jobs.cancel").params["job_id"], "host:late");
    assert!(!s.effective_visible());
    let out = s.handle_event(Event::new("job.finished", json!({"job_id": "host:late", "request_id": r.id, "ok": true, "result": {"asset_ref": "asset:late"}})));
    assert!(s.effective_visible());
    assert_eq!(s.resources.iter().count(), 0);
    assert_eq!(
        request(&out, "resources.release").params["asset_ref"],
        "asset:late"
    );
}

#[test]
fn stale_or_revoked_capture_never_imports_and_uses_host_job_for_cancel() {
    let mut s = session();
    let (_, r) = capture(&mut s);
    s.handle_response(Response::success(&r.id, json!({"job_id": "host:stale"})).unwrap());
    let p = context(&s);
    result(&call(&mut s, "pages.add", p));
    let out = s.handle_event(Event::new("job.finished", json!({"job_id": "host:stale", "ok": true, "result": {"asset_ref": "asset:stale", "total_bytes": 100, "crc32": 0}})));
    assert_eq!(done(&out)["error"]["code"], "revision_conflict");
    assert_eq!(s.resources.iter().count(), 0);
    assert!(
        out.iter()
            .all(|m| !matches!(m, Message::Request(r) if r.method == "resources.read"))
    );
    let (_, r) = capture(&mut s);
    s.handle_response(Response::success(&r.id, json!({"job_id": "host:revoke"})).unwrap());
    let out = call(
        &mut s,
        "configure",
        json!({"classroom_safe": true, "desktop_capture_allowed": false, "agent_allowed": false}),
    );
    assert_eq!(done(&out)["error"]["code"], "permission_revoked");
    assert_eq!(request(&out, "jobs.cancel").params["job_id"], "host:revoke");
    assert!(!s.effective_visible());
}

#[test]
fn all_windows_must_confirm_close_waits_and_disconnect_retains_unsaved_content() {
    let mut s = session();
    s.set_owned_windows(&["main", "tools"]).unwrap();
    let r = s.pending_window_request().unwrap().clone();
    assert!(
        s.acknowledge_windows(&r.request_id, &[("main", true)])
            .is_empty()
    );
    s.acknowledge_windows(&r.request_id, &[("main", true), ("tools", true)]);
    call(&mut s, "hide", json!({}));
    let r = s.pending_window_request().unwrap().clone();
    assert!(
        s.acknowledge_windows(&r.request_id, &[("main", false), ("tools", true)])
            .is_empty()
    );
    assert!(!s.window_confirmed);
    s.acknowledge_windows(&r.request_id, &[("main", false), ("tools", false)]);
    call(&mut s, "close", json!({}));
    assert!(!s.closed);
    assert!(s.close_pending);
    assert!(
        matches!(&call(&mut s, "show", json!({}))[0], Message::Response(r) if r.error.as_ref().unwrap().code == "session_closing")
    );
    ack(&mut s);
    assert!(s.closed);
    let mut s = session();
    let a = s.resources.import_png(png()).unwrap();
    image(&mut s, "unsaved", &a);
    capture(&mut s);
    let document = s.document.clone();
    let out = s.host_disconnected();
    assert!(!s.closed);
    assert_eq!(s.document, document);
    assert!(s.history.is_dirty(&s.document));
    assert!(s.resources.get(&a).is_some());
    assert!(!s.effective_visible());
    assert!(out.iter().all(|m| !matches!(m, Message::Request(_))));
    s.confirm_host_stopped();
    assert!(s.effective_visible());
    assert!(s.host_disconnected().is_empty());
}

#[test]
fn revoked_download_and_wrong_chunks_abort_without_importing() {
    for revoke in [true, false] {
        let mut s = session();
        let (id, r) = capture(&mut s);
        let bytes = png();
        let out = s.handle_response(Response::success(r.id, json!({"asset_ref": "asset:download", "total_bytes": bytes.len(), "crc32": resources::crc32(&bytes)})).unwrap());
        let read = request(&out, "resources.read");
        if revoke {
            let out = call(
                &mut s,
                "configure",
                json!({"classroom_safe": true, "desktop_capture_allowed": false, "agent_allowed": true}),
            );
            assert_eq!(done(&out)["error"]["code"], "permission_revoked");
            assert_eq!(
                request(&out, "resources.release").params["asset_ref"],
                "asset:download"
            );
            assert!(
                s.handle_response(Response::success(read.id, json!({})).unwrap())
                    .is_empty()
            );
        } else {
            let out = s.handle_response(Response::success(read.id, json!({"asset_ref": "asset:wrong", "offset": 0, "total_bytes": bytes.len(), "bytes": [1], "next_offset": 1, "eof": false})).unwrap());
            assert_eq!(done(&out)["error"]["code"], "invalid_host_response");
        }
        assert_eq!(s.resources.iter().count(), 0);
        assert!(!s.jobs.contains_key(&id));
        assert!(
            s.resources
                .begin_upload("test", resources::MAX_PNG_BYTES, 0)
                .is_ok()
        );
    }
}

#[test]
fn old_host_job_ids_cannot_complete_new_jobs_and_window_changes_cancel_capture() {
    let mut s = session();
    let (_, r) = capture(&mut s);
    s.handle_response(Response::success(r.id, json!({"job_id": "host:unique"})).unwrap());
    s.handle_event(Event::new("job.finished", json!({"job_id": "host:unique", "ok": false, "error": {"code": "cancelled", "message": "cancelled"}})));
    let (_, r) = capture(&mut s);
    let out = s.handle_response(Response::success(r.id, json!({"job_id": "host:unique"})).unwrap());
    assert_eq!(done(&out)["error"]["code"], "invalid_host_response");
    assert!(!s.effective_visible());
    assert!(
        s.handle_event(Event::new(
            "job.finished",
            json!({"job_id": "host:unique", "ok": true, "result": {"png_bytes": []}})
        ))
        .is_empty()
    );
    let mut s = session();
    capture(&mut s);
    let out = s.set_owned_windows(&["main", "new-dialog"]).unwrap();
    assert_eq!(done(&out)["error"]["code"], "window_set_changed");
    assert!(!s.effective_visible());
}

#[test]
fn upload_cleanup_on_permission_close_and_replacement() {
    for method in ["configure", "close", "document.new"] {
        let mut s = session();
        let upload = result(&call(
            &mut s,
            "resources.begin",
            json!({"total_bytes": 100, "crc32": 0}),
        ))["upload_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let params = if method == "configure" {
            json!({"classroom_safe": true, "desktop_capture_allowed": false, "agent_allowed": false})
        } else {
            json!({})
        };
        call(&mut s, method, params);
        assert_eq!(
            s.resources.abort_upload("neo", &upload).unwrap_err().code,
            "upload_not_found"
        );
    }
    let mut store = ResourceStore::default();
    let id = store.begin_upload("a", 100, 0).unwrap();
    assert_eq!(
        store.finish_upload("a", &id).unwrap_err().code,
        "resource_integrity"
    );
    assert_eq!(
        store.finish_upload("a", &id).unwrap_err().code,
        "upload_not_found"
    );
}

#[test]
fn cancellation_retries_have_unique_ids_and_stale_ack_cannot_unhide() {
    let mut s = session();
    let (id, capture) = capture(&mut s);
    let first = request(
        &call(&mut s, "jobs.cancel", json!({"job_id": id})),
        "jobs.cancel",
    );
    let second = request(
        &s.handle_response(
            Response::success(&capture.id, json!({"job_id": "host:later"})).unwrap(),
        ),
        "jobs.cancel",
    );
    assert_ne!(first.id, second.id);
    assert!(
        s.handle_response(Response::success(first.id, json!({"cancelled": true})).unwrap())
            .is_empty()
    );
    assert!(!s.effective_visible());
    assert!(
        s.handle_response(
            Response::success(
                &second.id,
                json!({"cancelled": true, "job_id": "host:wrong"})
            )
            .unwrap()
        )
        .is_empty()
    );
    assert!(!s.effective_visible());
    check(
        &s.handle_response(
            Response::success(
                second.id,
                json!({"cancelled": true, "job_id": "host:later"}),
            )
            .unwrap(),
        ),
    );
    assert!(s.effective_visible());
    assert!(s.cancelled_captures.is_empty());
}

#[test]
fn cancelled_capture_can_finish_before_pending_ack_with_local_job_identity() {
    let mut s = session();
    let (id, r) = capture(&mut s);
    call(&mut s, "jobs.cancel", json!({"job_id": id}));
    let event = Event::new(
        "job.finished",
        json!({"job_id": id, "ok": true,
        "result": {"asset_ref": "asset:late"}}),
    );
    assert!(s.handle_event(event.clone()).is_empty());
    assert!(!s.effective_visible());
    let mut event = event;
    event.data["request_id"] = json!(r.id);
    let out = s.handle_event(event.clone());
    check(&out);
    assert_eq!(
        request(&out, "resources.release").params["asset_ref"],
        "asset:late"
    );
    assert!(s.effective_visible());
    assert!(s.handle_event(event).is_empty());
}

#[test]
fn oversized_capture_event_keeps_lease_until_independent_cancel_ack() {
    let mut s = session();
    let (_, r) = capture(&mut s);
    s.handle_response(Response::success(r.id, json!({"job_id": "host:large"})).unwrap());
    let out = s.handle_event(Event::new(
        "job.finished",
        json!({"job_id": "host:large", "ok": true,
        "result": {"png_bytes": vec![255u8; MAX_LINE_BYTES]}}),
    ));
    assert_eq!(done(&out)["error"]["code"], "response_too_large");
    assert!(!s.effective_visible());
    assert!(s.jobs.is_empty());
    assert_eq!(s.resources.iter().count(), 0);
    let cancel = request(&out, "jobs.cancel");
    s.handle_response(Response::success(cancel.id, json!({"cancelled": true})).unwrap());
    assert!(s.effective_visible());
}

#[test]
fn package_orphans_duplicates_and_path_payloads_do_not_replace_live_resources() {
    let dir = std::env::temp_dir().join(format!("session-untrusted-{}", new_id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("document.json");
    let sentinel = dir.join("sentinel.png");
    std::fs::write(&sentinel, "不得覆盖".as_bytes()).unwrap();
    let mut s = session();
    let asset = s.resources.import_png(png()).unwrap();
    image(&mut s, "saved", &asset);
    s.save_document(&path).unwrap();
    let saved = std::fs::read(&path).unwrap();
    let package: Value = serde_json::from_slice(&saved).unwrap();
    let document = s.document.clone();
    for mode in 0..3 {
        let mut malformed = package.clone();
        let mut entry = malformed["resources"][0].clone();
        if mode == 0 {
            entry["asset_ref"] = json!("asset:orphan");
        }
        if mode == 2 {
            entry["asset_ref"] = json!(sentinel);
        }
        malformed["resources"].as_array_mut().unwrap().push(entry);
        std::fs::write(&path, serde_json::to_vec(&malformed).unwrap()).unwrap();
        assert_eq!(
            s.open_document(&path, true).unwrap_err().code,
            "invalid_resource"
        );
        assert_eq!(s.document, document);
        assert!(s.history.can_undo());
        assert!(!s.history.is_dirty(&s.document));
        assert_eq!(s.resources.iter().count(), 1);
        assert!(s.resources.get(&asset).is_some());
        assert_eq!(std::fs::read(&sentinel).unwrap(), "不得覆盖".as_bytes());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn eof_keeps_document_history_and_resources_saveable_and_clears_uploads() {
    let dir = std::env::temp_dir().join(format!("session-eof-{}", new_id()));
    std::fs::create_dir(&dir).unwrap();
    let mut s = session();
    let asset = s.resources.import_png(png()).unwrap();
    image(&mut s, "unsaved", &asset);
    let upload = s.resources.begin_upload("neo", 100, 0).unwrap();
    let mut input = std::io::Cursor::new(Vec::<u8>::new());
    assert!(board_protocol::read_message(&mut input).unwrap().is_none());
    check(&s.host_disconnected());
    assert!(!s.closed);
    assert!(s.history.can_undo());
    assert!(s.history.is_dirty(&s.document));
    assert_eq!(
        s.resources.abort_upload("neo", &upload).unwrap_err().code,
        "upload_not_found"
    );
    let path = dir.join("recovered.json");
    s.save_document(&path).unwrap();
    let mut reopened = session();
    reopened.open_document(path, false).unwrap();
    assert_eq!(reopened.document, s.document);
    assert_eq!(
        reopened.resources.png_bytes(&asset),
        s.resources.png_bytes(&asset)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn malformed_event_request_identity_cannot_finish_active_or_cancelled_capture() {
    for cancel in [false, true] {
        let mut s = session();
        let (id, r) = capture(&mut s);
        s.handle_response(Response::success(&r.id, json!({"job_id": "host:identity"})).unwrap());
        if cancel {
            call(&mut s, "jobs.cancel", json!({"job_id": id}));
        }
        for identity in [Value::Null, json!(7), json!("runtime:wrong")] {
            assert!(
                s.handle_event(Event::new(
                    "job.finished",
                    json!({"job_id": "host:identity",
                "request_id": identity, "ok": true, "result": {"asset_ref": "asset:untrusted"}})
                ))
                .is_empty()
            );
            assert!(!s.effective_visible());
        }
        check(&s.handle_event(Event::new(
            "job.finished",
            json!({"job_id": "host:identity",
            "request_id": r.id, "ok": false, "error": {"code": "cancelled", "message": "已停止"}}),
        )));
        assert!(s.effective_visible());
    }
}

#[test]
fn cancelling_capture_does_not_supersede_pending_close_response() {
    let mut s = session();
    let (_, r) = capture(&mut s);
    let out = call(&mut s, "close", json!({}));
    let cancel = request(&out, "jobs.cancel");
    let window = s.pending_window_request().unwrap().clone();
    s.handle_response(Response::success(cancel.id, json!({"cancelled": true})).unwrap());
    assert_eq!(
        s.pending_window_request().unwrap().request_id,
        window.request_id
    );
    assert_eq!(result(&ack(&mut s))["closed"], true);
    assert!(
        s.handle_response(Response::success(r.id, json!({"png_bytes": []})).unwrap())
            .is_empty()
    );
}
