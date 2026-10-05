use super::*;
use board_core::{BoardObject, Color, Point};

fn call(s: &mut Session, method: &str, params: Value) -> Vec<Message> {
    let out = s.handle(Request::new("neo:test", method, params).unwrap());
    for message in &out {
        board_protocol::write_message(&mut Vec::new(), message).unwrap();
    }
    out
}
fn reply(out: &[Message]) -> &Response {
    out.iter()
        .find_map(|m| {
            if let Message::Response(r) = m {
                Some(r)
            } else {
                None
            }
        })
        .unwrap()
}
fn ok(out: &[Message]) -> Value {
    let r = reply(out);
    assert!(r.ok, "{:?}", r.error);
    r.result.clone().unwrap()
}
fn code(out: &[Message]) -> &str {
    &reply(out).error.as_ref().unwrap().code
}
#[test]
fn ready_advertises_handwritten_and_version_three_without_new_rpc() {
    let s = Session::new(AppKind::Blackboard);
    let Message::Event(ready) = s.ready() else {
        panic!("ready event")
    };
    assert_eq!(ready.data["document_file_versions"], json!([1, 2, 3]));
    assert!(
        ready.data["resource_persistence_versions"]
            .as_array()
            .unwrap()
            .contains(&json!("board-session-package-v3"))
    );
    assert!(
        ready.data["object_types"]
            .as_array()
            .unwrap()
            .contains(&json!("handwritten"))
    );
    assert!(
        !ready.data["methods"]
            .as_array()
            .unwrap()
            .iter()
            .any(|method| method.as_str().unwrap().starts_with("handwriting."))
    );
}

fn configured() -> Session {
    let mut s = Session::new(AppKind::Drawing);
    ok(&call(
        &mut s,
        "configure",
        json!({"classroom_safe": false, "desktop_capture_allowed": true, "agent_allowed": true}),
    ));
    s
}
fn context(s: &Session) -> Value {
    json!({"document_id": s.document.id, "page_id": s.document.current_page().id, "expected_revision": s.document.revision})
}
fn object(id: &str, text: &str) -> BoardObject {
    BoardObject {
        id: id.into(),
        kind: ObjectKind::Text {
            position: Point::default(),
            text: text.into(),
            size: 20.0,
            color: Color::default(),
        },
    }
}
fn add(s: &mut Session, id: &str, text: &str) {
    let mut p = context(s);
    p["operations"] = json!([Operation::Add {
        object: object(id, text)
    }]);
    ok(&call(s, "objects.apply", p));
}
fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[255, 0, 0, 255]).unwrap();
    }
    bytes
}
fn acknowledge(s: &mut Session) -> Vec<Message> {
    let request = s.pending_window_request().unwrap().clone();
    s.acknowledge_window(&request.request_id, request.visible)
}
fn host_request(out: &[Message]) -> Request {
    out.iter()
        .find_map(|m| {
            if let Message::Request(r) = m {
                Some(r.clone())
            } else {
                None
            }
        })
        .unwrap()
}
fn finished(out: &[Message]) -> Value {
    out.iter()
        .find_map(|m| match m {
            Message::Event(e) if e.event == "job.finished" => Some(e.data.clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn configure_is_required_atomic_and_headless_is_truthful() {
    let mut s = Session::new(AppKind::Blackboard);
    assert_eq!(code(&call(&mut s, "show", json!({}))), "not_configured");
    assert_eq!(
        code(&call(&mut s, "configure", json!({"classroom_safe": false}))),
        "invalid_params"
    );
    assert!(!s.configured);
    assert_eq!(s.state()["visible"], false);
    assert_eq!(s.state()["hidden_confirmed"], false);
    let Message::Event(ready) = s.ready() else {
        panic!()
    };
    assert_eq!(ready.data["headless"], true);
    assert!(
        !ready.data["methods"]
            .as_array()
            .unwrap()
            .contains(&json!("capture.request"))
    );
    let mut s = configured();
    assert!(s.effective_visible());
    assert_eq!(
        ok(&call(&mut s, "hide", json!({})))["window_status"],
        "no_window"
    );
    assert_eq!(ok(&call(&mut s, "show", json!({})))["visible"], false);
}

#[test]
fn leases_preserve_user_intent_and_do_not_release_each_other() {
    let mut s = configured();
    ok(&call(&mut s, "window.suspend", json!({"lease_id": "a"})));
    ok(&call(&mut s, "window.suspend", json!({"lease_id": "b"})));
    assert!(!s.effective_visible());
    ok(&call(&mut s, "window.resume", json!({"lease_id": "a"})));
    assert!(!s.effective_visible());
    ok(&call(&mut s, "hide", json!({})));
    ok(&call(&mut s, "window.resume", json!({"lease_id": "b"})));
    assert!(!s.desired_visible);
    assert_eq!(
        code(&call(&mut s, "window.resume", json!({"lease_id": "b"}))),
        "lease_not_found"
    );
    ok(&call(&mut s, "show", json!({})));
    assert!(s.effective_visible());
}

#[test]
fn gui_hide_response_requires_matching_real_ack() {
    let mut s = configured();
    s.attach_window();
    acknowledge(&mut s);
    assert_eq!(s.state()["visible"], true);
    let out = call(&mut s, "hide", json!({}));
    assert!(!out.iter().any(|m| matches!(m, Message::Response(_))));
    assert_eq!(s.state()["visible"], true);
    assert!(s.acknowledge_window("stale", false).is_empty());
    let request = s.pending_window_request().unwrap().clone();
    assert!(s.acknowledge_window(&request.request_id, true).is_empty());
    let out = acknowledge(&mut s);
    assert_eq!(ok(&out)["hidden_confirmed"], true);
    assert_eq!(s.state()["visible"], false);
}

#[test]
fn atomic_apply_revision_and_undo_redo() {
    let mut s = configured();
    let original = s.document.clone();
    let mut p = context(&s);
    p["operations"] = json!([
        Operation::Add {
            object: object("a", "a")
        },
        Operation::Delete {
            id: "missing".into()
        }
    ]);
    assert_eq!(code(&call(&mut s, "objects.apply", p)), "invalid_document");
    assert_eq!(s.document, original);
    assert!(!s.history.can_undo());
    let stale = context(&s);
    add(&mut s, "a", "中文");
    let mut p = stale;
    p["operations"] = json!([]);
    assert_eq!(code(&call(&mut s, "objects.apply", p)), "revision_conflict");
    let p = context(&s);
    ok(&call(&mut s, "undo", p));
    assert!(s.document.current_page().objects.is_empty());
    assert!(!s.history.is_dirty(&s.document));
    let p = context(&s);
    ok(&call(&mut s, "redo", p));
    assert_eq!(s.document.revision, 3);
    assert_eq!(s.document.current_page().objects.len(), 1);
}

#[test]
fn unsaved_close_and_replacement_are_never_silent() {
    let mut s = configured();
    add(&mut s, "a", "保留");
    assert_eq!(code(&call(&mut s, "close", json!({}))), "unsaved_changes");
    assert!(!s.closed);
    assert_eq!(
        code(&call(&mut s, "document.new", json!({}))),
        "unsaved_changes"
    );
    assert_eq!(
        code(&call(
            &mut s,
            "document.open",
            json!({"path": "not-read.json"})
        )),
        "unsaved_changes"
    );
    ok(&call(&mut s, "close", json!({"discard_unsaved": true})));
    assert!(s.closed);
    assert!(!s.effective_visible());
    assert_eq!(code(&call(&mut s, "show", json!({}))), "session_closed");
}

#[test]
fn pages_share_history_and_select_does_not_dirty() {
    let mut s = configured();
    let first = s.document.current_page().id.clone();
    let p = context(&s);
    ok(&call(&mut s, "pages.add", p));
    let mut p = context(&s);
    p["page_id"] = json!(first);
    ok(&call(&mut s, "pages.select", p));
    assert_eq!(s.document.revision, 1);
    let p = context(&s);
    ok(&call(&mut s, "pages.delete", p));
    assert_eq!(s.document.pages.len(), 1);
    let p = context(&s);
    assert_eq!(code(&call(&mut s, "pages.delete", p)), "invalid_document");
    let p = context(&s);
    ok(&call(&mut s, "undo", p));
    assert_eq!(s.document.pages.len(), 2);
    let p = json!({"document_id": s.document.id});
    assert_eq!(
        ok(&call(&mut s, "pages.list", p))["pages"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn pagination_counts_utf8_and_envelope_bytes_and_rejects_stale_offsets() {
    let mut s = configured();
    for i in 0..12 {
        add(&mut s, &i.to_string(), &"中文\\\"\n".repeat(120));
    }
    let mut offset = 0;
    let mut count = 0;
    loop {
        let mut p = context(&s);
        p["offset"] = json!(offset);
        p["max_bytes"] = json!(4096);
        let out = call(&mut s, "objects.list", p);
        assert!(serde_json::to_vec(reply(&out)).unwrap().len() <= 4096);
        let result = ok(&out);
        count += result["objects"].as_array().unwrap().len();
        let Some(next) = result["next_offset"].as_u64() else {
            break;
        };
        assert!(next > offset);
        offset = next;
    }
    assert_eq!(count, 12);
    let mut stale = context(&s);
    stale["offset"] = json!(1);
    add(&mut s, "new", "new");
    assert_eq!(
        code(&call(&mut s, "objects.list", stale)),
        "revision_conflict"
    );
    let mut p = context(&s);
    p["max_bytes"] = json!(1024);
    assert_eq!(code(&call(&mut s, "objects.list", p)), "object_too_large");
}

#[test]
fn resource_import_read_release_is_memory_only() {
    let mut s = configured();
    assert_eq!(
        code(&call(
            &mut s,
            "resources.import_png",
            json!({"path": "secret.png"})
        )),
        "invalid_params"
    );
    assert_eq!(
        code(&call(
            &mut s,
            "resources.import_png",
            json!({"bytes": [1, 2, 3]})
        )),
        "invalid_png"
    );
    let bytes = png();
    let result = ok(&call(
        &mut s,
        "resources.import_png",
        json!({"bytes": bytes}),
    ));
    let id = result["asset_ref"].as_str().unwrap();
    let mut all = Vec::<u8>::new();
    let mut offset = 0;
    loop {
        let chunk = ok(&call(
            &mut s,
            "resources.read",
            json!({"asset_ref": id, "offset": offset, "length": 9}),
        ));
        all.extend(serde_json::from_value::<Vec<u8>>(chunk["bytes"].clone()).unwrap());
        offset = chunk["next_offset"].as_u64().unwrap();
        if chunk["eof"] == true {
            break;
        }
    }
    assert_eq!(all, bytes);
    assert_eq!(
        code(&call(
            &mut s,
            "resources.read",
            json!({"asset_ref": id, "offset": 0, "length": 8193})
        )),
        "invalid_params"
    );
    assert_eq!(
        ok(&call(&mut s, "resources.release", json!({"asset_ref": id})))["released"],
        true
    );
    assert_eq!(
        code(&call(
            &mut s,
            "resources.read",
            json!({"asset_ref": id, "offset": 0, "length": 9})
        )),
        "resource_not_found"
    );
}

#[test]
fn image_resource_cannot_be_released_or_falsely_saved() {
    let mut s = configured();
    let id = s.resources.import_png(png()).unwrap();
    let mut p = context(&s);
    p["operations"] = json!([Operation::Add {
        object: BoardObject {
            id: "image".into(),
            kind: ObjectKind::Image {
                position: Point::default(),
                width: 1.0,
                height: 1.0,
                asset_ref: id.clone()
            }
        }
    }]);
    ok(&call(&mut s, "objects.apply", p));
    assert_eq!(
        code(&call(&mut s, "resources.release", json!({"asset_ref": id}))),
        "resource_in_use"
    );
    let dir = std::env::temp_dir().join(format!("board-image-{}", new_id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("images.json");
    ok(&call(&mut s, "document.save", json!({"path": path})));
    assert!(!s.history.is_dirty(&s.document));
    let mut reopened = configured();
    ok(&call(&mut reopened, "document.open", json!({"path": path})));
    assert_eq!(reopened.document, s.document);
    assert_eq!(reopened.resources.png_bytes(&id), Some(png().as_slice()));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn capture_requires_authorization_permission_window_and_hidden_ack() {
    let mut s = configured();
    let mut p = context(&s);
    assert_eq!(
        code(&call(&mut s, "capture.request", p.clone())),
        "authorization_required"
    );
    p["user_authorized"] = json!(true);
    assert_eq!(
        code(&call(&mut s, "capture.request", p.clone())),
        "window_unavailable"
    );
    s.attach_window();
    acknowledge(&mut s);
    ok(&call(
        &mut s,
        "configure",
        json!({"classroom_safe": true, "desktop_capture_allowed": true, "agent_allowed": true}),
    ));
    assert_eq!(
        code(&call(&mut s, "capture.request", p.clone())),
        "permission_denied"
    );
    ok(&call(
        &mut s,
        "configure",
        json!({"classroom_safe": false, "desktop_capture_allowed": true, "agent_allowed": true}),
    ));
    let out = call(&mut s, "capture.request", p);
    let job = ok(&out)["job_id"].clone();
    assert!(!out.iter().any(|m| matches!(m, Message::Request(_))));
    assert!(!s.effective_visible());
    let out = acknowledge(&mut s);
    let request = host_request(&out);
    assert_eq!(request.method, "host.capture_region");
    assert_eq!(request.params["windows_hidden_confirmed"], true);
    let out =
        s.handle_response(Response::success(request.id, json!({"png_bytes": png()})).unwrap());
    let done = finished(&out);
    assert_eq!(done["job_id"], job);
    assert_eq!(done["ok"], true);
    assert!(
        s.resources
            .get(done["result"]["asset_ref"].as_str().unwrap())
            .is_some()
    );
    assert!(s.effective_visible());
    assert!(s.pending_window_request().unwrap().visible);
    assert!(
        !out.iter()
            .any(|m| matches!(m, Message::Request(r) if r.method == "host.ask_agent"))
    );
}

fn agent(s: &mut Session, write_back: bool) -> (Value, Request) {
    let mut p = context(s);
    p["user_authorized"] = json!(true);
    p["prompt"] = json!("解释");
    p["write_back"] = json!(write_back);
    let out = call(s, "agent.request", p);
    (ok(&out)["job_id"].clone(), host_request(&out))
}

#[test]
fn asynchronous_writeback_validates_revision_and_is_atomic() {
    let mut s = configured();
    let (_, request) = agent(&mut s, true);
    add(&mut s, "local", "本地修改");
    let out = s.handle_response(
        Response::success(
            request.id,
            json!({"operations": [Operation::Add { object: object("remote", "远端") }]}),
        )
        .unwrap(),
    );
    assert_eq!(finished(&out)["error"]["code"], "revision_conflict");
    assert_eq!(s.document.current_page().objects.len(), 1);
    let (_, request) = agent(&mut s, true);
    let out = s.handle_response(Response::success(request.id, json!({"answer": "结果", "operations": [Operation::Add { object: object("remote", "远端") }]})).unwrap());
    assert_eq!(finished(&out)["ok"], true);
    assert_eq!(s.document.current_page().objects.len(), 2);
    let (_, request) = agent(&mut s, false);
    let out = s.handle_response(Response::success(request.id, json!({"operations": []})).unwrap());
    assert_eq!(finished(&out)["error"]["code"], "permission_denied");
}

#[test]
fn cancellation_and_permission_revocation_ignore_late_replies() {
    let mut s = configured();
    let (job, request) = agent(&mut s, true);
    let out = call(&mut s, "jobs.cancel", json!({"job_id": job}));
    assert_eq!(finished(&out)["error"]["code"], "cancelled");
    assert_eq!(host_request(&out).method, "jobs.cancel");
    assert!(
        s.handle_response(Response::success(request.id, json!({"operations": []})).unwrap())
            .is_empty()
    );
    let (_, request) = agent(&mut s, true);
    let out = call(
        &mut s,
        "configure",
        json!({"classroom_safe": true, "desktop_capture_allowed": false, "agent_allowed": false}),
    );
    assert_eq!(finished(&out)["error"]["code"], "permission_revoked");
    assert!(
        s.handle_response(Response::success(request.id, json!({"answer": "迟到"})).unwrap())
            .is_empty()
    );
}

#[test]
fn save_open_atomic_failure_preserves_document_and_dirty_state() {
    // 临时文件只用于显式文档方法测试，测试完成后清理。
    let dir = std::env::temp_dir().join(format!("board-session-{}", new_id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("board.json");
    let mut s = configured();
    add(&mut s, "a", "保存前");
    ok(&call(&mut s, "document.save", json!({"path": path})));
    assert!(!s.history.is_dirty(&s.document));
    let saved = std::fs::read(&path).unwrap();
    add(&mut s, "b", "保存后");
    assert_eq!(
        code(&call(&mut s, "document.save", json!({"path": dir}))),
        "io_error"
    );
    assert!(s.history.is_dirty(&s.document));
    assert_eq!(std::fs::read(&path).unwrap(), saved);
    let previous = s.document.clone();
    assert_eq!(
        code(&call(
            &mut s,
            "document.open",
            json!({"path": dir.join("missing.json"), "discard_unsaved": true})
        )),
        "io_error"
    );
    assert_eq!(s.document, previous);
    ok(&call(
        &mut s,
        "document.open",
        json!({"path": path, "discard_unsaved": true}),
    ));
    assert_eq!(s.document.current_page().objects.len(), 1);
    assert!(!s.history.is_dirty(&s.document));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cancelling_active_capture_keeps_hidden_until_host_stops() {
    let mut s = configured();
    s.attach_window();
    acknowledge(&mut s);
    let mut p = context(&s);
    p["user_authorized"] = json!(true);
    let job = ok(&call(&mut s, "capture.request", p))["job_id"].clone();
    let capture = host_request(&acknowledge(&mut s));
    let out = call(&mut s, "jobs.cancel", json!({"job_id": job}));
    let cancel = host_request(&out);
    assert!(!s.effective_visible());
    assert!(s.pending_window_request().is_none());
    let out = s.handle_response(Response::success(cancel.id, json!({"cancelled": true})).unwrap());
    assert!(s.effective_visible());
    assert!(s.pending_window_request().unwrap().visible);
    assert!(
        !out.iter()
            .any(|m| matches!(m, Message::Event(e) if e.event == "job.finished"))
    );
    assert!(
        s.handle_response(Response::success(capture.id, json!({"png_bytes": png()})).unwrap())
            .is_empty()
    );
}

#[test]
fn all_advertised_mutations_require_configuration() {
    let mut s = Session::new(AppKind::Drawing);
    let Message::Event(ready) = s.ready() else {
        panic!()
    };
    let before = s.document.clone();
    for method in ready.data["methods"].as_array().unwrap() {
        let method = method.as_str().unwrap();
        if matches!(method, "configure" | "get_state" | "close") {
            continue;
        }
        assert_eq!(
            code(&call(&mut s, method, json!({}))),
            "not_configured",
            "{method}"
        );
    }
    assert_eq!(s.document, before);
    assert_eq!(s.resources.iter().count(), 0);
    assert!(s.jobs.is_empty());
    assert!(s.hide_leases.is_empty());
}

#[test]
fn oversized_object_can_be_read_losslessly_and_listing_can_continue() {
    let mut s = configured();
    let large = object("large", &"中文\\\"\n".repeat(14000));
    let page = s.document.current_page().id.clone();
    let revision = s.document.revision;
    s.history
        .apply(
            &mut s.document,
            &page,
            revision,
            &[Operation::Add {
                object: large.clone(),
            }],
        )
        .unwrap();
    add(&mut s, "small", "后续对象");
    let mut p = context(&s);
    p["max_bytes"] = json!(MAX_LINE_BYTES);
    let out = call(&mut s, "objects.list", p);
    let e = reply(&out).error.as_ref().unwrap();
    assert_eq!(e.code, "object_too_large");
    let data = e.data.as_ref().unwrap();
    assert_eq!(data["object_id"], "large");
    assert_eq!(data["next_offset"], 1);
    let mut bytes = Vec::<u8>::new();
    let mut p = context(&s);
    p["object_id"] = data["object_id"].clone();
    p["length"] = json!(8192);
    loop {
        p["offset"] = json!(bytes.len());
        let chunk = ok(&call(&mut s, "objects.read", p.clone()));
        bytes.extend(serde_json::from_value::<Vec<u8>>(chunk["bytes"].clone()).unwrap());
        assert_eq!(chunk["next_offset"], bytes.len());
        if chunk["eof"] == true {
            break;
        }
    }
    assert!(bytes.len() > MAX_LINE_BYTES);
    assert_eq!(
        serde_json::from_slice::<BoardObject>(&bytes).unwrap(),
        large
    );
    let mut next = context(&s);
    next["offset"] = data["next_offset"].clone();
    assert_eq!(
        ok(&call(&mut s, "objects.list", next))["objects"][0]["id"],
        "small"
    );
    let mut invalid = p.clone();
    invalid["length"] = json!(8193);
    assert_eq!(
        code(&call(&mut s, "objects.read", invalid)),
        "invalid_params"
    );
    add(&mut s, "changed", "版本变化");
    assert_eq!(code(&call(&mut s, "objects.read", p)), "revision_conflict");
}

#[test]
fn five_hundred_pages_are_paginated_without_gaps_and_revision_is_required() {
    let mut s = configured();
    for _ in 1..500 {
        let p = context(&s);
        ok(&call(&mut s, "pages.add", p));
    }
    let mut ids = Vec::new();
    loop {
        let p = json!({"document_id": s.document.id, "expected_revision": s.document.revision,
            "offset": ids.len(), "limit": 37, "max_bytes": 1024});
        let out = call(&mut s, "pages.list", p);
        assert!(serde_json::to_vec(reply(&out)).unwrap().len() <= 1024);
        let value = ok(&out);
        assert_eq!(value["total"], 500);
        ids.extend(
            value["pages"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v["page_id"].as_str().unwrap().to_owned()),
        );
        if value["next_offset"].is_null() {
            break;
        }
        assert_eq!(value["next_offset"], ids.len());
    }
    assert_eq!(
        ids,
        s.document
            .pages
            .iter()
            .map(|p| p.id.clone())
            .collect::<Vec<_>>()
    );
    let p = json!({"document_id": s.document.id, "offset": 1});
    assert_eq!(code(&call(&mut s, "pages.list", p)), "invalid_params");
    let stale = json!({"document_id": s.document.id, "offset": 1, "expected_revision": s.document.revision});
    let p = context(&s);
    ok(&call(&mut s, "pages.delete", p));
    assert_eq!(
        code(&call(&mut s, "pages.list", stale)),
        "revision_conflict"
    );
}

#[test]
fn oversized_agent_response_finishes_once_without_writeback() {
    let mut s = configured();
    let (_, r) = agent(&mut s, true);
    let before = s.document.clone();
    let out = s.handle_response(
        Response::success(
            &r.id,
            json!({"answer": "x".repeat(MAX_LINE_BYTES),
        "operations": [Operation::Add { object: object("remote", "不可写入") }]}),
        )
        .unwrap(),
    );
    assert_eq!(finished(&out)["error"]["code"], "response_too_large");
    for m in &out {
        board_protocol::write_message(&mut Vec::new(), m).unwrap();
    }
    assert!(s.jobs.is_empty());
    assert_eq!(s.document, before);
    assert!(
        s.handle_response(Response::success(r.id, json!({"answer": "迟到"})).unwrap())
            .is_empty()
    );
    let (_, r) = agent(&mut s, false);
    assert_eq!(
        finished(&s.handle_response(Response::success(r.id, json!({"answer": "仍可用"})).unwrap()))
            ["ok"],
        true
    );
}

#[test]
fn bounded_response_fallback_and_large_answer_do_not_partially_commit() {
    let message = response(
        "neo:large",
        Ok(json!({"text": "中文".repeat(MAX_LINE_BYTES)})),
    );
    board_protocol::write_message(&mut Vec::new(), &message).unwrap();
    assert_eq!(code(&[message]), "response_too_large");
    let mut s = configured();
    let (_, r) = agent(&mut s, true);
    let before = s.document.clone();
    let incoming = Response::success(
        r.id,
        json!({"answer": "x".repeat(MAX_LINE_BYTES - 1900),
        "operations": [Operation::Add { object: object("remote", "不能部分完成") }]}),
    )
    .unwrap();
    // 此帧本身可传输，但回答加完成事件封装已超出保守预算。
    board_protocol::write_message(&mut Vec::new(), &Message::Response(incoming.clone())).unwrap();
    let out = s.handle_response(incoming);
    assert_eq!(finished(&out)["error"]["code"], "response_too_large");
    assert_eq!(s.document, before);
    assert!(!s.history.can_undo());
}

fn connection_fixture() -> Session {
    use board_core::{ShapeKind, Style};
    let mut s = configured();
    let mut p = context(&s);
    let shapes = [
        ("line", ShapeKind::Line, vec![(0.0, 0.0), (20.0, 20.0)]),
        (
            "rect",
            ShapeKind::Rectangle,
            vec![(2.0, 4.0), (10.0, 4.0), (10.0, 12.0), (2.0, 12.0)],
        ),
        (
            "cube",
            ShapeKind::Cube,
            vec![(1.0, 1.0), (5.0, 1.0), (5.0, 5.0)],
        ),
    ];
    p["operations"] = json!(
        shapes
            .into_iter()
            .map(|(id, shape, coordinates)| {
                Operation::Add {
                    object: BoardObject {
                        id: id.into(),
                        kind: ObjectKind::Shape {
                            shape,
                            points: coordinates
                                .into_iter()
                                .map(|(x, y)| Point { x, y })
                                .collect(),
                            style: Style::default(),
                        },
                    },
                }
            })
            .collect::<Vec<_>>()
    );
    ok(&call(&mut s, "objects.apply", p));
    s
}
fn connection_params(s: &Session, id: &str, endpoint: usize, target: &str, anchor: Value) -> Value {
    let mut p = context(s);
    p["connection"] = json!({"id": id, "page_id": p["page_id"], "line_id": "line",
        "line_endpoint": endpoint, "target_id": target, "target": anchor});
    p
}
fn line_points(s: &Session) -> Vec<Point> {
    let object = s.document.pages[0]
        .objects
        .iter()
        .find(|o| o.id == "line")
        .unwrap();
    let ObjectKind::Shape { points, .. } = &object.kind else {
        panic!()
    };
    points.clone()
}

#[test]
fn connections_follow_move_delete_undo_redo_and_disconnect_atomically() {
    let mut s = connection_fixture();
    let original_points = line_points(&s);
    let revision = s.document.revision;
    let p = connection_params(
        &s,
        "c",
        0,
        "rect",
        json!({"type": "edge", "start": 0, "end": 1, "t": 0.25}),
    );
    let out = call(&mut s, "connections.connect", p);
    assert_eq!(ok(&out)["revision"], revision + 1);
    assert!(
        out.iter()
            .any(|m| matches!(m, Message::Event(e) if e.event == "document_changed"))
    );
    assert_eq!(line_points(&s)[0], Point { x: 4.0, y: 4.0 });
    assert_eq!(s.document.connections.len(), 1);
    let p = context(&s);
    assert!(ok(&call(&mut s, "undo", p))["changed"].as_bool().unwrap());
    assert!(s.document.connections.is_empty());
    assert_eq!(line_points(&s), original_points);
    let p = context(&s);
    ok(&call(&mut s, "redo", p));
    assert_eq!(s.document.connections.len(), 1);

    let mut moved = s.document.pages[0]
        .objects
        .iter()
        .find(|o| o.id == "rect")
        .unwrap()
        .clone();
    let ObjectKind::Shape { points, .. } = &mut moved.kind else {
        panic!()
    };
    for point in points {
        point.x += 30.0;
        point.y += 40.0;
    }
    let mut p = context(&s);
    p["operations"] = json!([Operation::Update { object: moved }]);
    let revision = s.document.revision;
    ok(&call(&mut s, "objects.apply", p));
    assert_eq!(s.document.revision, revision + 1);
    let followed = line_points(&s);
    assert_eq!(followed[0], Point { x: 34.0, y: 44.0 });
    let p = context(&s);
    let listed = ok(&call(&mut s, "objects.list", p));
    assert_eq!(
        listed["objects"][0]["kind"]["points"][0],
        json!(followed[0])
    );

    let mut p = context(&s);
    p["connection_id"] = json!("c");
    ok(&call(&mut s, "connections.disconnect", p));
    assert!(s.document.connections.is_empty());
    assert_eq!(line_points(&s), followed);
    let p = context(&s);
    ok(&call(&mut s, "undo", p));
    assert_eq!(s.document.connections.len(), 1);
    let p = context(&s);
    ok(&call(&mut s, "redo", p));
    assert!(s.document.connections.is_empty());
    let p = context(&s);
    ok(&call(&mut s, "undo", p));

    for id in ["rect", "line"] {
        let before = s.document.clone();
        let mut p = context(&s);
        p["operations"] = json!([Operation::Delete { id: id.into() }]);
        ok(&call(&mut s, "objects.apply", p));
        assert!(s.document.connections.is_empty());
        assert_eq!(s.document.revision, before.revision + 1);
        let p = context(&s);
        ok(&call(&mut s, "undo", p));
        assert_eq!(s.document.connections, before.connections);
        assert_eq!(s.document.pages, before.pages);
        let p = context(&s);
        ok(&call(&mut s, "redo", p));
        assert!(s.document.connections.is_empty());
        let p = context(&s);
        ok(&call(&mut s, "undo", p));
    }
}

#[test]
fn connections_reject_invalid_anchors_mixed_dimensions_and_stale_context_without_history_changes() {
    let mut s = connection_fixture();
    let p = connection_params(&s, "c", 0, "rect", json!({"type": "vertex", "index": 0}));
    ok(&call(&mut s, "connections.connect", p));
    let p = context(&s);
    ok(&call(&mut s, "undo", p));
    assert!(s.history.can_redo());
    for anchor in [
        json!({"type": "vertex", "index": 1000}),
        json!({"type": "edge", "start": 0, "end": 0, "t": 0.5}),
        json!({"type": "edge", "start": 0, "end": 1, "t": 1.1}),
    ] {
        let before = s.document.clone();
        let p = connection_params(&s, "bad", 0, "rect", anchor);
        let out = call(&mut s, "connections.connect", p);
        assert_eq!(code(&out), "invalid_document");
        assert_eq!(out.len(), 1);
        assert_eq!(s.document, before);
        assert!(s.history.can_redo());
    }
    let p = context(&s);
    ok(&call(&mut s, "redo", p));
    let before = s.document.clone();
    let p = connection_params(
        &s,
        "mixed",
        1,
        "cube",
        json!({"type": "vertex", "index": 0}),
    );
    assert_eq!(
        code(&call(&mut s, "connections.connect", p)),
        "invalid_document"
    );
    assert_eq!(s.document, before);
    for (field, value, expected) in [
        ("document_id", json!("other"), "document_mismatch"),
        ("page_id", json!("missing"), "page_not_found"),
        ("expected_revision", json!(0), "revision_conflict"),
    ] {
        for method in ["connections.connect", "connections.disconnect"] {
            let mut p =
                connection_params(&s, "c2", 1, "rect", json!({"type": "vertex", "index": 0}));
            p["connection_id"] = json!("c");
            p[field] = value.clone();
            assert_eq!(code(&call(&mut s, method, p)), expected);
            assert_eq!(s.document, before);
        }
    }
    let mut p = connection_params(&s, "c2", 1, "rect", json!({"type": "vertex", "index": 0}));
    p["connection"]["page_id"] = json!("other");
    assert_eq!(
        code(&call(&mut s, "connections.connect", p)),
        "invalid_document"
    );
    let mut p = context(&s);
    p["connection_id"] = json!("missing");
    assert_eq!(
        code(&call(&mut s, "connections.disconnect", p)),
        "invalid_document"
    );
    assert_eq!(s.document, before);
}

#[test]
fn connections_ready_permissions_page_filter_and_frame_budget_pagination() {
    let mut s = Session::new(AppKind::Blackboard);
    let Message::Event(ready) = s.ready() else {
        panic!()
    };
    for method in [
        "connections.list",
        "connections.connect",
        "connections.disconnect",
    ] {
        assert!(
            ready.data["methods"]
                .as_array()
                .unwrap()
                .contains(&json!(method))
        );
        assert_eq!(code(&call(&mut s, method, json!({}))), "not_configured");
    }
    let mut s = connection_fixture();
    ok(&call(
        &mut s,
        "configure",
        json!({"classroom_safe": true,
        "desktop_capture_allowed": false, "agent_allowed": false}),
    ));
    for endpoint in 0..2 {
        let p = connection_params(
            &s,
            &format!("c{endpoint}{}", "x".repeat(80)),
            endpoint,
            "rect",
            json!({"type": "vertex", "index": endpoint}),
        );
        ok(&call(&mut s, "connections.connect", p));
    }
    let request_id = format!("neo:{}", "\\\"".repeat(126));
    let mut p = context(&s);
    p["max_bytes"] = json!(1024);
    let out = s.handle(Request::new(&request_id, "connections.list", p.clone()).unwrap());
    let first = ok(&out);
    assert_eq!(first["connections"].as_array().unwrap().len(), 1);
    assert_eq!(first["total"], 2);
    assert_eq!(first["next_offset"], 1);
    assert!(serde_json::to_vec(reply(&out)).unwrap().len() <= 1024);
    p["offset"] = first["next_offset"].clone();
    let out = s.handle(Request::new(&request_id, "connections.list", p.clone()).unwrap());
    assert_eq!(ok(&out)["connections"][0], json!(s.document.connections[1]));
    assert_eq!(ok(&out)["next_offset"], Value::Null);
    assert!(serde_json::to_vec(reply(&out)).unwrap().len() <= 1024);
    p.as_object_mut().unwrap().remove("expected_revision");
    assert_eq!(
        code(&call(&mut s, "connections.list", p.clone())),
        "invalid_params"
    );
    p["expected_revision"] = json!(0);
    assert_eq!(
        code(&call(&mut s, "connections.list", p)),
        "revision_conflict"
    );
    let p = context(&s);
    let page = ok(&call(&mut s, "pages.add", p))["page_id"].clone();
    let mut p = context(&s);
    p["page_id"] = page;
    assert_eq!(
        ok(&call(&mut s, "connections.list", p))["connections"],
        json!([])
    );
}

#[test]
fn app_history_edits_are_visible_to_connection_rpc_and_share_undo() {
    let mut s = connection_fixture();
    let p = connection_params(
        &s,
        "local",
        0,
        "rect",
        json!({"type": "vertex", "index": 0}),
    );
    let connection: Connection = serde_json::from_value(p["connection"].clone()).unwrap();
    let page = s.document.current_page().id.clone();
    let revision = s.document.revision;
    s.history
        .edit(&mut s.document, |d| d.connect(&page, revision, connection))
        .unwrap();
    let p = context(&s);
    assert_eq!(
        ok(&call(&mut s, "connections.list", p))["connections"],
        json!(s.document.connections)
    );
    let mut p = context(&s);
    p["connection_id"] = json!("local");
    ok(&call(&mut s, "connections.disconnect", p));
    let p = context(&s);
    ok(&call(&mut s, "undo", p));
    assert_eq!(s.document.connections[0].id, "local");
    let p = context(&s);
    ok(&call(&mut s, "undo", p));
    assert!(s.document.connections.is_empty());
    assert_eq!(line_points(&s)[0], Point::default());
}

#[test]
fn math_plain_and_image_package_versions_roundtrip_without_loss() {
    for with_image in [false, true] {
        let mut s = configured();
        let object = BoardObject {
            id: "math-result".into(),
            kind: ObjectKind::Math {
                position: Point { x: 10.0, y: 20.0 },
                layout: board_core::MathLayout::Fraction(
                    Box::new(board_core::MathLayout::Radical(Box::new(
                        board_core::MathLayout::Text("2".into()),
                    ))),
                    Box::new(board_core::MathLayout::Text("-3".into())),
                ),
                size: 24.0,
                color: Color::default(),
            },
        };
        let mut p = context(&s);
        p["operations"] = json!([Operation::Add {
            object: object.clone()
        }]);
        ok(&call(&mut s, "objects.apply", p));
        let asset = if with_image {
            let asset = ok(&call(
                &mut s,
                "resources.import_png",
                json!({"bytes": png()}),
            ))["asset_ref"]
                .as_str()
                .unwrap()
                .to_owned();
            let mut p = context(&s);
            p["operations"] = json!([Operation::Add {
                object: BoardObject {
                    id: "image".into(),
                    kind: ObjectKind::Image {
                        position: Point::default(),
                        width: 10.0,
                        height: 10.0,
                        asset_ref: asset.clone(),
                    }
                }
            }]);
            ok(&call(&mut s, "objects.apply", p));
            Some(asset)
        } else {
            None
        };
        let path = std::env::temp_dir().join(format!("session-math-{}.json", new_id()));
        ok(&call(&mut s, "document.save", json!({"path": path})));
        let source = std::fs::read_to_string(&path).unwrap();
        let mut value: Value = serde_json::from_str(&source).unwrap();
        assert_eq!(value["version"], 2);
        if with_image {
            assert_eq!(value["format"], "board-session-package");
        }
        let mut restored = configured();
        ok(&call(&mut restored, "document.open", json!({"path": path})));
        assert_eq!(restored.document, s.document);
        assert_eq!(restored.document.current_page().objects[0], object);
        assert!(!restored.history.is_dirty(&restored.document));
        if let Some(asset) = asset {
            assert_eq!(restored.resources.png_bytes(&asset).unwrap(), png());
        }
        let before = restored.document.clone();
        for version in [1, 99] {
            value["version"] = json!(version);
            std::fs::write(&path, value.to_string()).unwrap();
            let error = package::load(&path).unwrap_err();
            assert_eq!(
                error.code,
                if version == 1 {
                    "invalid_document"
                } else {
                    "unsupported_version"
                }
            );
            assert_eq!(restored.document, before);
        }
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn handwritten_plain_and_image_package_roundtrip_rejects_v2_without_replacing_session() {
    for with_image in [false, true] {
        for with_layout in [false, true] {
            let mut s = configured();
            let object = BoardObject {
                id: "handwritten-result".into(),
                kind: ObjectKind::Handwritten {
                    position: Point { x: 80.0, y: 120.0 },
                    text: "2".into(),
                    layout: with_layout.then(|| board_core::MathLayout::Text("2".into())),
                    strokes: vec![board_core::HandwritingStroke {
                        points: vec![
                            board_core::StrokePoint {
                                x: 1.0,
                                y: 2.0,
                                time: 0.0,
                                pressure: 1.0,
                            },
                            board_core::StrokePoint {
                                x: 12.0,
                                y: 24.0,
                                time: 0.25,
                                pressure: 1.0,
                            },
                        ],
                        style: board_core::Style {
                            color: Color {
                                r: 32,
                                g: 64,
                                b: 128,
                                a: 200,
                            },
                            width: 2.5,
                            dashed: false,
                        },
                    }],
                },
            };
            let mut p = context(&s);
            p["operations"] = json!([Operation::Add {
                object: object.clone()
            }]);
            ok(&call(&mut s, "objects.apply", p));
            let asset = if with_image {
                let asset = ok(&call(
                    &mut s,
                    "resources.import_png",
                    json!({"bytes": png()}),
                ))["asset_ref"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                let mut p = context(&s);
                p["operations"] = json!([Operation::Add {
                    object: BoardObject {
                        id: "image".into(),
                        kind: ObjectKind::Image {
                            position: Point::default(),
                            width: 10.0,
                            height: 10.0,
                            asset_ref: asset.clone(),
                        },
                    },
                }]);
                ok(&call(&mut s, "objects.apply", p));
                Some(asset)
            } else {
                None
            };
            let path = std::env::temp_dir().join(format!("session-handwritten-{}.json", new_id()));
            ok(&call(&mut s, "document.save", json!({"path": path})));
            assert!(!s.history.is_dirty(&s.document));
            let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(value["version"], 3);
            assert_eq!(value["document"]["pages"][0]["objects"][0], json!(object));
            if with_image {
                assert_eq!(value["format"], "board-session-package");
                assert_eq!(value["resources"].as_array().unwrap().len(), 1);
            } else {
                assert!(value.get("format").is_none());
                assert!(value.get("resources").is_none());
            }

            // A fresh session has no personal profile; the object carries its frozen ink.
            let mut restored = configured();
            ok(&call(&mut restored, "document.open", json!({"path": path})));
            assert_eq!(restored.document, s.document);
            assert_eq!(restored.document.current_page().objects[0], object);
            assert!(!restored.history.is_dirty(&restored.document));
            assert!(!restored.history.can_undo());
            assert!(!restored.history.can_redo());
            if let Some(asset) = &asset {
                assert_eq!(restored.resources.png_bytes(asset).unwrap(), png());
            }

            add(&mut restored, "unsaved", "keep this edit");
            add(&mut restored, "redoable", "keep this redo");
            let p = context(&restored);
            ok(&call(&mut restored, "undo", p));
            let before = restored.document.clone();
            let state = ok(&call(&mut restored, "get_state", json!({})));
            assert_eq!(state["dirty"], true);
            assert_eq!(state["can_undo"], true);
            assert_eq!(state["can_redo"], true);
            value["version"] = json!(2);
            std::fs::write(&path, value.to_string()).unwrap();
            assert_eq!(
                code(&call(
                    &mut restored,
                    "document.open",
                    json!({"path": path, "discard_unsaved": true}),
                )),
                "invalid_document"
            );
            assert_eq!(restored.document, before);
            assert_eq!(ok(&call(&mut restored, "get_state", json!({}))), state);
            if let Some(asset) = &asset {
                assert_eq!(restored.resources.png_bytes(asset).unwrap(), png());
            }
            let p = context(&restored);
            assert_eq!(ok(&call(&mut restored, "redo", p))["changed"], true);
            assert_eq!(
                restored.document.current_page().objects.last().unwrap().id,
                "redoable"
            );
            for _ in 0..2 {
                let p = context(&restored);
                assert_eq!(ok(&call(&mut restored, "undo", p))["changed"], true);
            }
            assert_eq!(
                restored.document.current_page().objects,
                s.document.current_page().objects
            );
            assert!(!restored.history.is_dirty(&restored.document));
            std::fs::remove_file(path).unwrap();
        }
    }
}

#[test]
fn connections_plain_and_resource_package_roundtrip() {
    for with_resource in [false, true] {
        let mut s = connection_fixture();
        let p = connection_params(&s, "c", 0, "rect", json!({"type": "vertex", "index": 0}));
        ok(&call(&mut s, "connections.connect", p));
        let asset = if with_resource {
            let asset = ok(&call(
                &mut s,
                "resources.import_png",
                json!({"bytes": png()}),
            ))["asset_ref"]
                .as_str()
                .unwrap()
                .to_owned();
            let mut p = context(&s);
            p["operations"] = json!([Operation::Add {
                object: BoardObject {
                    id: "image".into(),
                    kind: ObjectKind::Image {
                        asset_ref: asset.clone(),
                        position: Point::default(),
                        width: 10.0,
                        height: 10.0
                    }
                }
            }]);
            ok(&call(&mut s, "objects.apply", p));
            Some(asset)
        } else {
            None
        };
        let path = std::env::temp_dir().join(format!("session-connections-{}.json", new_id()));
        ok(&call(&mut s, "document.save", json!({"path": path})));
        let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["version"], 1);
        assert!(!s.history.is_dirty(&s.document));
        let saved = s.document.clone();
        let mut restored = configured();
        ok(&call(&mut restored, "document.open", json!({"path": path})));
        std::fs::remove_file(path).unwrap();
        assert_eq!(restored.document, saved);
        assert!(!restored.history.can_undo());
        assert!(!restored.history.is_dirty(&restored.document));
        if let Some(asset) = asset {
            assert_eq!(restored.resources.png_bytes(&asset).unwrap(), png());
        }
        let p = context(&restored);
        assert_eq!(
            ok(&call(&mut restored, "connections.list", p))["connections"],
            json!(saved.connections)
        );
        let mut p = context(&restored);
        p["connection_id"] = json!("c");
        ok(&call(&mut restored, "connections.disconnect", p));
        let p = context(&restored);
        ok(&call(&mut restored, "undo", p));
        assert!(!restored.history.is_dirty(&restored.document));
    }
}

#[test]
fn headless_list_error_empty_page_and_oversized_state_frames_are_bounded() {
    let mut s = connection_fixture();
    let p = connection_params(
        &s,
        &"x".repeat(2000),
        0,
        "rect",
        json!({"type": "vertex", "index": 0}),
    );
    ok(&call(&mut s, "connections.connect", p));
    let mut p = context(&s);
    p["max_bytes"] = json!(1024);
    let out = call(&mut s, "connections.list", p.clone());
    assert_eq!(code(&out), "response_too_large");
    assert!(serde_json::to_vec(reply(&out)).unwrap().len() <= 1024);
    p["max_bytes"] = json!(4096);
    assert_eq!(ok(&call(&mut s, "connections.list", p))["total"], 1);

    // 文件或GUI可产生长ID；空列表和错误帧也必须遵守max_bytes。
    s.document.id = "d".repeat(2000);
    s.history = History::new(&s.document);
    let mut p = context(&s);
    p["offset"] = json!(1);
    p["max_bytes"] = json!(1024);
    let out = call(&mut s, "connections.list", p);
    assert_eq!(code(&out), "response_too_large");
    assert!(serde_json::to_vec(reply(&out)).unwrap().len() <= 1024);

    s.document.id = "d".repeat(MAX_LINE_BYTES);
    s.history = History::new(&s.document);
    let out = call(&mut s, "get_state", json!({}));
    assert_eq!(code(&out), "response_too_large");
    let out = call(&mut s, "hide", json!({}));
    assert_eq!(code(&out), "response_too_large");
    assert!(out.iter().any(|m| matches!(m, Message::Event(e)
        if e.event == "protocol_error" && e.data["code"] == "event_too_large")));
    assert!(!s.desired_visible);
    ok(&call(
        &mut s,
        "document.new",
        json!({"discard_unsaved": true}),
    ));
    assert_eq!(
        ok(&call(&mut s, "get_state", json!({})))["has_window"],
        false
    );
}

#[test]
fn oversized_host_error_keeps_correlated_job_finished() {
    let mut s = configured();
    let (job, request) = agent(&mut s, false);
    let mut incoming = Response::failure(&request.id, error("host_error", "")).unwrap();
    let overhead = serde_json::to_vec(&incoming).unwrap().len();
    incoming.error.as_mut().unwrap().message = "x".repeat(MAX_LINE_BYTES - overhead);
    assert_eq!(serde_json::to_vec(&incoming).unwrap().len(), MAX_LINE_BYTES);
    let out = s.handle_response(incoming);
    for message in &out {
        board_protocol::write_message(&mut Vec::new(), message).unwrap();
    }
    let completion = finished(&out);
    assert_eq!(completion["job_id"], job);
    assert_eq!(completion["error"]["code"], "response_too_large");
    assert!(s.jobs.is_empty());
}

#[test]
fn configure_safe_is_independent_of_agent_and_invalid_changes_are_atomic() {
    let mut s = configured();
    s.attach_window();
    acknowledge(&mut s);
    ok(&call(
        &mut s,
        "configure",
        json!({"classroom_safe": true,
        "desktop_capture_allowed": true, "agent_allowed": true}),
    ));
    let (_, request) = agent(&mut s, false);
    let before = s.state();
    for params in [
        json!({}),
        json!({"classroom_safe": false,
        "desktop_capture_allowed": true, "agent_allowed": 1}),
        json!({"classroom_safe": false, "desktop_capture_allowed": true,
        "agent_allowed": true, "unknown": false}),
    ] {
        assert_eq!(code(&call(&mut s, "configure", params)), "invalid_params");
        assert_eq!(s.state(), before);
    }
    let mut p = context(&s);
    p["user_authorized"] = json!(true);
    assert_eq!(
        code(&call(&mut s, "capture.request", p)),
        "permission_denied"
    );
    let out = s.handle_response(Response::success(request.id, json!({"answer": "ok"})).unwrap());
    assert_eq!(finished(&out)["ok"], true);
}

#[test]
fn request_and_window_input_budgets_reject_without_side_effects() {
    let mut s = configured();
    let before = s.state();
    for id in [
        format!("neo:{}", "x".repeat(253)),
        format!("neo:{}", "中".repeat(85)),
    ] {
        let out = s.handle(Request::new(id, "configure", json!({})).unwrap());
        assert!(matches!(&out[0], Message::Event(e) if e.data["code"] == "invalid_id"));
        assert_eq!(s.state(), before);
    }
    let mut r = Request::new("neo:budget", "configure", json!({"padding": ""})).unwrap();
    let overhead = serde_json::to_vec(&r).unwrap().len();
    r.params["padding"] = json!("x".repeat(MAX_LINE_BYTES - overhead + 1));
    assert_eq!(code(&s.handle(r)), "line_too_long");
    assert_eq!(s.state(), before);
    assert!(s.set_owned_windows(&["main"; 129]).is_err());
    assert!(s.set_owned_windows(&[&"x".repeat(MAX_LINE_BYTES)]).is_err());
    assert_eq!(s.state(), before);
    s.attach_window();
    let pending = s.pending_window_request().unwrap().clone();
    assert!(
        s.acknowledge_windows(&pending.request_id, &[("main", false); 129])
            .is_empty()
    );
    assert_eq!(s.pending_window_request(), Some(&pending));
}

#[test]
fn pending_cancellations_share_job_limit_across_document_and_permission_changes() {
    let mut s = configured();
    s.attach_window();
    acknowledge(&mut s);
    for _ in 0..32 {
        let mut p = context(&s);
        p["user_authorized"] = json!(true);
        ok(&call(&mut s, "capture.request", p));
        acknowledge(&mut s);
    }
    let mut p = context(&s);
    p["user_authorized"] = json!(true);
    p["prompt"] = json!("explain");
    assert_eq!(code(&call(&mut s, "agent.request", p)), "job_limit");
    let mut out = call(&mut s, "document.new", json!({"discard_unsaved": true}));
    assert!(s.jobs.is_empty());
    assert_eq!(s.cancelled_captures.len(), 32);
    assert_eq!(s.hide_leases.len(), 32);
    out.extend(call(
        &mut s,
        "configure",
        json!({"classroom_safe": true,
        "desktop_capture_allowed": false, "agent_allowed": true}),
    ));
    let mut p = context(&s);
    p["user_authorized"] = json!(true);
    p["prompt"] = json!("explain");
    assert_eq!(code(&call(&mut s, "agent.request", p)), "job_limit");
    let cancellations: Vec<_> = out
        .iter()
        .filter_map(|m| match m {
            Message::Request(r) if r.method == "jobs.cancel" => Some(r.id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(cancellations.len(), 32);
    for id in cancellations {
        out.extend(s.handle_response(Response::success(id, json!({"cancelled": true})).unwrap()));
    }
    for message in out {
        board_protocol::write_message(&mut Vec::new(), &message).unwrap();
    }
    assert!(s.cancelled_captures.is_empty());
    assert!(s.hide_leases.is_empty());
    let (_, request) = agent(&mut s, false);
    let out = s.handle_response(
        Response::success(request.id, json!({"job_id": "x".repeat(257)})).unwrap(),
    );
    assert_eq!(finished(&out)["error"]["code"], "invalid_host_response");
    assert!(s.jobs.is_empty());
    assert!(s.seen_host_jobs.is_empty());
}

#[test]
fn advertised_methods_are_implemented_and_headless_omits_capture() {
    let mut s = configured();
    for with_window in [false, true] {
        if with_window {
            s.attach_window();
            acknowledge(&mut s);
        }
        let Message::Event(ready) = s.ready() else {
            panic!()
        };
        let methods = ready.data["methods"].as_array().unwrap();
        assert_eq!(methods.contains(&json!("capture.request")), with_window);
        for method in methods {
            let method = method.as_str().unwrap();
            if method == "close" {
                continue;
            }
            let out = call(&mut s, method, json!({}));
            for message in out {
                if let Message::Response(r) = message {
                    assert!(
                        r.error.is_none_or(|e| e.code != "method_not_found"),
                        "{method}"
                    );
                }
            }
        }
    }
}

#[test]
fn deterministic_memory_only_requests_preserve_session_invariants() {
    for seed in [1u32, 0x92fa_734b, 0x7fff_ffff] {
        let mut random = seed;
        let mut s = configured();
        s.attach_window();
        acknowledge(&mut s);
        for step in 0..400 {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            let mut p = context(&s);
            let choice = random % 20;
            let method = match choice {
                0 => {
                    p["operations"] = json!([Operation::Add {
                        object: object(&format!("o{step}"), "中文\\\"\n")
                    }]);
                    "objects.apply"
                }
                1 => {
                    p["operations"] = json!([Operation::Delete {
                        id: "missing".into()
                    }]);
                    "objects.apply"
                }
                2 => "undo",
                3 => "redo",
                4 => "pages.add",
                5 => "pages.delete",
                6 => {
                    p["limit"] = json!(1);
                    p["max_bytes"] = json!(1024);
                    "objects.list"
                }
                7 => {
                    p = json!({"classroom_safe": random & 32 != 0,
                    "desktop_capture_allowed": random & 64 != 0, "agent_allowed": random & 128 != 0});
                    "configure"
                }
                8 => {
                    p = json!({"discard_unsaved": true});
                    "document.new"
                }
                9 => {
                    p["user_authorized"] = json!(false);
                    "capture.request"
                }
                10 => {
                    p["user_authorized"] = json!(true);
                    "capture.request"
                }
                11 => {
                    p["user_authorized"] = json!(true);
                    p["prompt"] = json!("explain");
                    "agent.request"
                }
                12 => {
                    p = json!({"job_id": s.jobs.keys().min().cloned().unwrap_or_default()});
                    "jobs.cancel"
                }
                13 => {
                    p = json!({"lease_id": "test-lease"});
                    "window.suspend"
                }
                14 => {
                    p = json!({"lease_id": "test-lease"});
                    "window.resume"
                }
                15 => {
                    p = json!({"bytes": png()});
                    "resources.import_png"
                }
                16 => {
                    p = json!({"total_bytes": if random & 32 == 0 { 17 } else { u64::MAX }, "crc32": 0});
                    "resources.begin"
                }
                17 => {
                    p["max_bytes"] = json!(1024);
                    "connections.list"
                }
                18 => {
                    p["user_authorized"] = json!(true);
                    p["prompt"] = json!("x".repeat(MAX_LINE_BYTES));
                    "agent.request"
                }
                _ => "get_state",
            };
            let prior_jobs = s.jobs.len();
            let prior_leases = s.hide_leases.len();
            let mut out = call(&mut s, method, p);
            if choice == 9 {
                assert_eq!(s.jobs.len(), prior_jobs);
                assert_eq!(s.hide_leases.len(), prior_leases);
            }
            if let Some(pending) = s.pending_window_request().cloned() {
                out.extend(s.acknowledge_window(&pending.request_id, pending.visible));
            }
            let requests: Vec<_> = out
                .iter()
                .filter_map(|m| match m {
                    Message::Request(r) => Some(r.clone()),
                    _ => None,
                })
                .collect();
            for request in requests {
                if request.method == "host.capture_region" {
                    assert!(s.permissions.desktop_capture_allowed && !s.permissions.classroom_safe);
                    assert!(s.has_window && s.window_confirmed && !s.actual_visible);
                    assert_eq!(request.params["user_authorized"], true);
                    assert_eq!(request.params["windows_hidden_confirmed"], true);
                }
                if random & 256 != 0 {
                    let reply = match request.method.as_str() {
                        "host.ask_agent" => {
                            Response::success(&request.id, json!({"answer": "ok"})).unwrap()
                        }
                        "jobs.cancel" => {
                            Response::success(&request.id, json!({"cancelled": true})).unwrap()
                        }
                        _ => Response::failure(&request.id, error("cancelled", "stopped")).unwrap(),
                    };
                    out.extend(s.handle_response(reply));
                }
            }
            for message in out {
                board_protocol::write_message(&mut Vec::new(), &message).unwrap();
                assert!(serde_json::to_vec(&message).unwrap().len() <= MAX_LINE_BYTES);
            }
            s.document.validate().unwrap();
            assert!(s.jobs.len() + s.cancelled_captures.len() <= 32);
            assert!(s.resources.iter().count() <= 256);
            assert!(
                s.resources
                    .iter()
                    .map(|(_, r)| r.bytes.len())
                    .sum::<usize>()
                    <= resources::MAX_STORE_BYTES
            );
            for job in s.jobs.values() {
                if let Some(lease) = &job.lease {
                    assert!(s.hide_leases.contains(lease));
                }
            }
            for cancelled in s.cancelled_captures.values() {
                assert!(s.hide_leases.contains(&cancelled.lease));
            }
        }
        for message in s.host_disconnected() {
            board_protocol::write_message(&mut Vec::new(), &message).unwrap();
        }
        assert!(s.jobs.is_empty());
        s.document.validate().unwrap();
    }
}

#[test]
fn math_is_real_and_unknown_methods_are_rejected() {
    let mut s = configured();
    assert_eq!(
        ok(&call(
            &mut s,
            "math.calculate",
            json!({"expression": "2+3*4"})
        ))["result"],
        "14"
    );
    assert_eq!(
        code(&call(
            &mut s,
            "math.calculate",
            json!({"expression": "1/0"})
        )),
        "math_error"
    );
    assert_eq!(
        code(&call(&mut s, "not.implemented", json!({}))),
        "method_not_found"
    );
}
