fn capture_app() -> BoardApp {
    let mut app = app();
    app.session.attach_window();
    app.incoming_message(Request::new("neo:allow", "configure", serde_json::json!({"classroom_safe": false, "desktop_capture_allowed": true, "agent_allowed": true})).unwrap().into());
    let pending = app.session.pending_window_request().unwrap().clone();
    let out = app.session.acknowledge_window(&pending.request_id, true);
    app.messages(out);
    app
}

fn start_capture(app: &mut BoardApp) -> (String, Request) {
    let request = app.take_host_request(true).unwrap();
    let out = app.session.handle(request);
    app.messages(out);
    let job = app.jobs.last().unwrap().0.clone();
    let pending = app.session.pending_window_request().unwrap().clone();
    assert!(!pending.visible);
    let out = app.session.acknowledge_window(&pending.request_id, false);
    let request = out
        .iter()
        .find_map(|m| match m {
            Message::Request(r) if r.method == "host.capture_region" => Some(r.clone()),
            _ => None,
        })
        .unwrap();
    app.messages(out);
    (job, request)
}

fn capture_png() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 200, 100);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&vec![255; 200 * 100 * 4])
            .unwrap();
    }
    bytes
}

#[test]
fn capture_direct_and_chunk_completion_insert_once_with_atomic_undo_and_order() {
    for path in ["direct", "event", "chunked"] {
        let mut app = capture_app();
        app.canvas_size = Vec2::new(180.0, 180.0);
        let (job, host) = start_capture(&mut app);
        let png = capture_png();
        app.emitted.clear();
        if path == "direct" {
            app.incoming_message(
                board_protocol::Response::success(
                    &host.id,
                    serde_json::json!({"png_bytes": png}),
                )
                .unwrap()
                .into(),
            );
        } else if path == "chunked" {
            let mut crc = !0u32;
            for byte in &png {
                crc ^= *byte as u32;
                for _ in 0..8 {
                    crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
                }
            }
            app.incoming_message(board_protocol::Response::success(&host.id, serde_json::json!({"asset_ref": "asset:hostpng", "total_bytes": png.len(), "crc32": !crc})).unwrap().into());
            assert!(app.session.document.current_page().objects.is_empty());
            assert_eq!(app.session.state()["hide_lease_count"], 0);
            loop {
                let read = app
                    .emitted
                    .iter()
                    .rev()
                    .find_map(|m| match m {
                        Message::Request(r) if r.method == "resources.read" => Some(r.clone()),
                        _ => None,
                    })
                    .unwrap();
                let offset = read.params["offset"].as_u64().unwrap() as usize;
                let end = (offset + (read.params["length"].as_u64().unwrap() as usize).min(31))
                    .min(png.len());
                app.incoming_message(board_protocol::Response::success(&read.id, serde_json::json!({"asset_ref": "asset:hostpng", "offset": offset, "total_bytes": png.len(), "bytes": png[offset..end], "next_offset": end, "eof": end == png.len()})).unwrap().into());
                if end == png.len() {
                    break;
                }
            }
        } else {
            app.incoming_message(
                board_protocol::Response::success(
                    &host.id,
                    serde_json::json!({"job_id": "host-capture"}),
                )
                .unwrap()
                .into(),
            );
            app.incoming_message(Event::new("job.finished", serde_json::json!({"job_id": "host-capture", "ok": true, "result": {"png_bytes": png}})).into());
        }
        assert!(!app.files && app.captured_asset.is_none() && app.capture_jobs.is_empty());
        assert!(
            matches!(&app.session.document.current_page().objects[0].kind, ObjectKind::Image { position, width, height, .. } if *width == 100.0 && *height == 50.0 && position.x == 40.0 && position.y == 65.0)
        );
        assert_eq!(app.session.document.revision, 1);
        let states: Vec<_> = app
            .emitted
            .iter()
            .filter_map(|m| match m {
                Message::Event(e) if e.event == "state_changed" => e.data["revision"].as_u64(),
                _ => None,
            })
            .collect();
        assert!(states.windows(2).all(|w| w[0] <= w[1]));
        assert!(
            !app.emitted
                .iter()
                .any(|m| matches!(m, Message::Request(r) if r.method == "host.ask_agent"))
        );
        app.messages(vec![Event::new("job.finished", serde_json::json!({"job_id": job, "ok": true, "result": {"asset_ref": "asset:hostpng"}})).into()]);
        app.incoming_message(
            board_protocol::Response::success(
                &host.id,
                serde_json::json!({"png_bytes": capture_png()}),
            )
            .unwrap()
            .into(),
        );
        assert_eq!(app.session.document.revision, 1);
        app.undo(false);
        assert!(app.session.document.current_page().objects.is_empty());
        app.undo(true);
        assert_eq!(app.session.document.current_page().objects.len(), 1);
    }
}

#[test]
fn capture_stale_page_fallback_and_unrelated_assets_never_insert() {
    let mut app = capture_app();
    app.session.document.add_page().unwrap();
    app.session.document.set_current_page(0).unwrap();
    let (_, host) = start_capture(&mut app);
    let doc = app.session.document.id.clone();
    let page = app.session.document.pages[1].id.clone();
    app.incoming_message(
        Request::new(
            "neo:page",
            "pages.select",
            serde_json::json!({"document_id": doc, "page_id": page}),
        )
        .unwrap()
        .into(),
    );
    app.incoming_message(
        board_protocol::Response::success(
            &host.id,
            serde_json::json!({"png_bytes": capture_png()}),
        )
        .unwrap()
        .into(),
    );
    assert!(
        app.session
            .document
            .pages
            .iter()
            .all(|p| p.objects.is_empty())
    );
    assert!(app.captured_asset.is_some() && app.status.contains("手动插入"));
    let asset = app.captured_asset.take().unwrap();
    app.messages(vec![Event::new("job.finished", serde_json::json!({"job_id": "agent-job", "ok": true, "result": {"asset_ref": asset}})).into()]);
    assert!(app.captured_asset.is_none() && !app.files);
}

#[test]
fn native_entry_close_restores_geometry_and_does_not_discard_dirty_document() {
    for dirty in [false, true] {
        let mut app = app();
        if dirty {
            app.add(ObjectKind::Text {
                position: Point { x: 0.0, y: 0.0 },
                text: "keep".into(),
                size: 20.0,
                color: Color::default(),
            });
        }
        let before = app.session.document.clone();
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        let vp = input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap();
        vp.inner_rect = Some(Rect::from_min_size(
            Pos2::new(40.0, 50.0),
            Vec2::new(900.0, 600.0),
        ));
        vp.outer_rect = vp.inner_rect;
        vp.maximized = Some(true);
        let mut out = ctx.run_ui(input, |_| app.set_collapsed(&ctx, true));
        out.textures_delta.clear();
        let mut input = egui::RawInput::default();
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .events
            .push(egui::ViewportEvent::Close);
        let mut out = ctx.run_ui(input, |ui| app.board_ui(ui, &ctx));
        out.textures_delta.clear();
        let commands = &out.viewport_output[&egui::ViewportId::ROOT].commands;
        if dirty {
            assert!(app.confirm_close && !app.allow_close);
            assert!(commands.contains(&ViewportCommand::CancelClose));
            assert!(commands.contains(&ViewportCommand::InnerSize(Vec2::new(900.0, 600.0))));
            assert!(commands.contains(&ViewportCommand::OuterPosition(Pos2::new(40.0, 50.0))));
            assert!(commands.contains(&ViewportCommand::Maximized(true)));
        } else {
            assert!(app.allow_close);
            assert!(commands.contains(&ViewportCommand::Close));
        }
        assert_eq!(app.session.document, before);
    }
}

#[test]
fn external_capture_and_real_agent_asset_field_do_not_auto_insert() {
    let mut app = capture_app();
    let asset = app.session.resources.import_png(capture_png()).unwrap();
    let request = Request::new("neo:external-capture", "capture.request", serde_json::json!({
        "document_id": app.session.document.id, "page_id": app.session.document.current_page().id,
        "expected_revision": app.session.document.revision, "user_authorized": true
    })).unwrap();
    app.incoming_message(request.into());
    let pending = app.session.pending_window_request().unwrap().clone();
    let out = app.session.acknowledge_window(&pending.request_id, false);
    let host = out
        .iter()
        .find_map(|m| match m {
            Message::Request(r) if r.method == "host.capture_region" => Some(r.clone()),
            _ => None,
        })
        .unwrap();
    app.messages(out);
    app.incoming_message(
        board_protocol::Response::success(&host.id, serde_json::json!({"asset_ref": asset}))
            .unwrap()
            .into(),
    );
    app.agent_prompt = "question".into();
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
    app.incoming_message(
        board_protocol::Response::success(
            &host.id,
            serde_json::json!({"answer": "answer", "asset_ref": asset}),
        )
        .unwrap()
        .into(),
    );
    assert!(app.session.document.current_page().objects.is_empty());
    assert!(app.captured_asset.is_none() && !app.files);
}

#[test]
fn capture_cancel_failure_revision_and_replacement_never_insert() {
    for reason in ["cancel", "failure", "revision", "document"] {
        let mut app = capture_app();
        let (job, host) = start_capture(&mut app);
        match reason {
            "cancel" => {
                app.cancel_job(&job);
                assert_eq!(app.session.state()["hide_lease_count"], 1);
            }
            "revision" => app.add(ObjectKind::Text {
                position: Point { x: 0.0, y: 0.0 },
                text: "new".into(),
                size: 24.0,
                color: Color::default(),
            }),
            "document" => app.incoming_message(
                Request::new(
                    "neo:new",
                    "document.new",
                    serde_json::json!({"discard_unsaved": true}),
                )
                .unwrap()
                .into(),
            ),
            _ => {}
        }
        let result = if reason == "failure" {
            serde_json::json!({"png_bytes": [1, 2, 3]})
        } else {
            serde_json::json!({"png_bytes": capture_png()})
        };
        app.incoming_message(
            board_protocol::Response::success(&host.id, result)
                .unwrap()
                .into(),
        );
        assert!(
            app.session
                .document
                .current_page()
                .objects
                .iter()
                .all(|o| !matches!(o.kind, ObjectKind::Image { .. }))
        );
        assert!(app.capture_jobs.is_empty() && app.captured_asset.is_none() && !app.files);
    }
}

#[test]
fn collapse_does_not_acknowledge_hidden_capture_lease() {
    let mut app = capture_app();
    let ctx = egui::Context::default();
    app.set_collapsed(&ctx, true);
    assert_eq!(app.session.state()["visible"], true);
    assert_eq!(app.session.state()["hidden_confirmed"], false);
    let request = app.take_host_request(true).unwrap();
    let out = app.session.handle(request);
    assert!(
        !out.iter()
            .any(|m| matches!(m, Message::Request(r) if r.method == "host.capture_region"))
    );
    app.messages(out);
    assert_eq!(app.session.state()["hidden_confirmed"], false);
    let pending = app.session.pending_window_request().unwrap().clone();
    assert!(!pending.visible);
    let out = app.session.acknowledge_window(&pending.request_id, false);
    assert!(
        out.iter()
            .any(|m| matches!(m, Message::Request(r) if r.method == "host.capture_region"))
    );
}

#[test]
fn toolbar_small_screen_and_collapsed_controls_remain_reachable() {
    for size in [
        Vec2::new(320.0, 240.0),
        Vec2::new(800.0, 600.0),
        Vec2::new(1920.0, 1080.0),
    ] {
        let mut app = app();
        let ctx = egui::Context::default();
        for collapsed in [false, true, false] {
            app.collapsed = collapsed;
            for _ in 0..3 {
                frame(&mut app, &ctx, size, vec![], true);
            }
            let screen = Rect::from_min_size(Pos2::ZERO, size).expand(1.0);
            assert_eq!(app.controls.len(), if collapsed { 1 } else { 3 });
            assert!(app.controls.iter().all(|r| screen.contains_rect(*r)));
            let bar = app.controls[0];
            assert!(screen.contains_rect(bar), "{size:?}: {bar:?}");
            if !collapsed {
                let scale = toolbar_scale(size, false);
                let height = 44.0 * scale + 2.0 * (6.0 * scale).round();
                assert!(
                    (bar.height() - height).abs() < 1.0,
                    "single row: {size:?}: {bar:?}"
                );
                assert!(
                    bar.width() >= 100.0 * scale,
                    "toolbar lost main tools: {bar:?}"
                );
                assert!(
                    app.controls[1].right() + 4.0 < bar.left(),
                    "{size:?}: {:?}",
                    app.controls
                );
                assert!(bar.right() + 4.0 < app.controls[2].left());
            }
        }
    }
}
