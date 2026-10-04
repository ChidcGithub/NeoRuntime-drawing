#[test]
fn each_image_has_its_own_button_and_only_that_asset_is_authorized() {
    let mut app = app();
    for (id, x) in [("a", 20.0), ("b", 240.0)] {
        app.session.document.pages[0].objects.push(BoardObject {
            id: id.into(),
            kind: ObjectKind::Image {
                position: Point { x, y: 60.0 },
                width: 160.0,
                height: 100.0,
                asset_ref: format!("asset:{id}"),
            },
        });
    }
    let ctx = egui::Context::default();
    let size = Vec2::new(640.0, 360.0);
    let render = |app: &mut BoardApp, events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |_| {
                app.controls.clear();
                app.plot_controls(&ctx);
            },
        );
        output.textures_delta.clear();
    };
    render(&mut app, vec![]);
    render(&mut app, vec![]);
    assert_eq!(app.controls.len(), 2);
    let pos = app.controls[0].center();
    render(
        &mut app,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
    );
    render(&mut app, vec![pointer(pos, false)]);
    assert_eq!(app.agent_image.as_ref().unwrap().1, "a");
    assert_eq!(app.authorization, Some(false));
    assert!(app.authorized_asset_refs().is_empty());
    app.authorize_assets = true;
    assert_eq!(app.authorized_asset_refs(), vec!["asset:a"]);
    assert!(!app.authorize_write_back);
    app.authorize_write_back = true;
    let pos = app.controls[1].center();
    render(
        &mut app,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
    );
    render(&mut app, vec![pointer(pos, false)]);
    assert_eq!(app.agent_image.as_ref().unwrap().1, "b");
    assert!(!app.authorize_write_back);
    assert!(!app.authorize_assets);
    assert_eq!(app.session.state()["permissions"]["agent_allowed"], false);
    assert!(app.jobs.is_empty());
}

#[test]
fn writeback_is_opt_in_single_use_and_independent_of_assets() {
    for allowed in [false, true] {
        let mut app = app();
        assert!(!app.authorize_write_back);
        app.agent_prompt = " 写出步骤 ".into();
        app.authorize_write_back = allowed;
        let request = app.take_host_request(false).unwrap();
        assert_eq!(request.method, "agent.request");
        assert_eq!(request.params["write_back"], allowed);
        assert_eq!(request.params["asset_refs"], serde_json::json!([]));
        assert_eq!(request.params["prompt"], "写出步骤");
        assert!(!app.authorize_write_back);
        app.session.handle(Request::new("neo:allow", "configure", serde_json::json!({"classroom_safe": true, "desktop_capture_allowed": false, "agent_allowed": true})).unwrap());
        let out = app.session.handle(request);
        let host = out
            .iter()
            .find_map(|m| match m {
                Message::Request(r) if r.method == "host.ask_agent" => Some(r),
                _ => None,
            })
            .unwrap();
        assert_eq!(host.params["write_back"], allowed);
        assert_eq!(host.params["revision"], app.session.document.revision);
        assert!(host.params.get("expected_revision").is_none());
        assert_eq!(
            app.take_host_request(false).unwrap().params["write_back"],
            false
        );
    }
    let mut app = app();
    app.session.document.pages[0].objects.push(BoardObject {
        id: "image".into(),
        kind: ObjectKind::Image {
            position: Point::default(),
            width: 10.0,
            height: 10.0,
            asset_ref: "asset:local".into(),
        },
    });
    app.authorize_assets = true;
    let request = app.take_host_request(false).unwrap();
    assert_eq!(
        request.params["asset_refs"],
        serde_json::json!(["asset:local"])
    );
    assert_eq!(request.params["write_back"], false);
    assert!(!app.authorize_assets);
}

#[test]
fn cancelled_or_changed_context_clears_writeback() {
    for change in 0..5 {
        let mut app = app();
        app.session
            .history
            .edit(&mut app.session.document, |d| d.add_page())
            .unwrap();
        app.authorization = Some(false);
        app.authorize_write_back = true;
        app.authorize_assets = true;
        match change {
            0 => app.cancel_authorization(),
            1 => {
                app.session.document.set_current_page(0).unwrap();
                app.changed();
            }
            2 => app.incoming_message(
                Request::new(
                    "neo:select",
                    "pages.select",
                    serde_json::json!({"document_id": app.session.document.id,
                    "page_id": app.session.document.pages[0].id}),
                )
                .unwrap()
                .into(),
            ),
            3 => app.add(text_kind("local edit")),
            _ => {
                let ctx = egui::Context::default();
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        events: vec![egui::Event::Key {
                            key: Key::Escape,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: Default::default(),
                        }],
                        ..Default::default()
                    },
                    |_| app.shortcuts(&ctx),
                );
                output.textures_delta.clear();
            }
        }
        assert!(!app.authorize_write_back, "change {change}");
        assert!(!app.authorize_assets);
        assert!(app.authorization.is_none());
    }
    let mut app = app();
    app.authorize_write_back = true;
    app.request_host(false);
    assert!(!app.authorize_write_back);
    assert!(app.jobs.is_empty());
    assert!(app.status.contains("未发送"));
}

#[test]
fn stale_or_unauthorized_writeback_is_rejected_in_ui_without_partial_changes() {
    for stale in [false, true] {
        for asynchronous in [false, true] {
            let mut app = app();
            app.session.handle(Request::new("neo:allow", "configure", serde_json::json!({"classroom_safe": true, "desktop_capture_allowed": false, "agent_allowed": true})).unwrap());
            app.agent_prompt = "步骤".into();
            app.authorize_write_back = stale;
            let request = app.take_host_request(false).unwrap();
            let out = app.session.handle(request);
            let host = out
                .iter()
                .find_map(|m| match m {
                    Message::Request(r) if r.method == "host.ask_agent" => Some(r.clone()),
                    _ => None,
                })
                .unwrap();
            app.messages(out);
            if asynchronous {
                app.incoming_message(
                    board_protocol::Response::success(
                        &host.id,
                        serde_json::json!({"job_id": "host-job"}),
                    )
                    .unwrap()
                    .into(),
                );
            }
            if stale {
                app.add(text_kind("newer edit"));
            }
            let before = app.session.document.clone();
            let result = serde_json::json!({"answer": "steps", "operations": [
                {"op": "add", "object": {"id": "step1", "kind": text_kind("step1")}},
                {"op": "add", "object": {"id": "step2", "kind": text_kind("step2")}}
            ]});
            if asynchronous {
                app.incoming_message(
                    Event::new(
                        "job.finished",
                        serde_json::json!({
                            "job_id": "host-job", "ok": true, "result": result
                        }),
                    )
                    .into(),
                );
            } else {
                app.incoming_message(
                    board_protocol::Response::success(&host.id, result)
                        .unwrap()
                        .into(),
                );
            }
            assert_eq!(app.session.document, before);
            assert!(app.jobs.is_empty());
            assert!(app.status.contains(if stale {
                "revision_conflict"
            } else {
                "permission_denied"
            }));
        }
    }
}

#[test]
fn save_failure_keeps_dirty_and_reports_error() {
    let mut app = app();
    app.add(ObjectKind::Text {
        position: Point::default(),
        text: "保留".into(),
        size: 24.0,
        color: Color::default(),
    });
    app.path = "\0invalid".into();
    app.confirm_close = true;
    assert!(!app.save());
    assert!(!app.status.is_empty());
    assert!(app.session.history.is_dirty(&app.session.document));
    assert!(!app.allow_close);
}

#[test]
fn authorization_popup_stays_on_small_screen_at_high_dpi() {
    for scale in [1.0, 2.0] {
        let mut app = app();
        app.authorization = Some(false);
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(scale);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 240.0));
        for _ in 0..4 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |_| {
                    app.controls.clear();
                    app.authorization_panel(&ctx);
                },
            );
            output.textures_delta.clear();
        }
        assert_eq!(app.controls.len(), 1);
        assert!(
            screen.expand(1.0).contains_rect(app.controls[0]),
            "{:?}",
            app.controls[0]
        );
    }
}

#[test]
fn cancelling_host_job_uses_session_and_removes_ui_entry() {
    let mut app = app();
    app.session.handle(Request::new("neo:allow", "configure", serde_json::json!({"classroom_safe": true, "desktop_capture_allowed": false, "agent_allowed": true})).unwrap());
    let params = serde_json::json!({"document_id": app.session.document.id, "page_id": app.session.document.current_page().id, "expected_revision": app.session.document.revision, "user_authorized": true, "prompt": "test", "write_back": false});
    let out = app
        .session
        .handle(Request::new("runtime:gui:test", "agent.request", params).unwrap());
    app.messages(out);
    assert_eq!(app.jobs.len(), 1);
    let id = app.jobs[0].0.clone();
    app.authorize_write_back = true;
    app.cancel_job(&id);
    assert!(!app.authorize_write_back);
    assert!(app.jobs.is_empty());
    assert_eq!(app.session.state()["pending_jobs"], 0);
    assert!(app.status.contains("cancelled"));
}
