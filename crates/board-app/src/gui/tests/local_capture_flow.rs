fn local_capture_app() -> BoardApp {
    let mut app = app();
    app.session.attach_window();
    let pending = app.session.pending_window_request().unwrap().clone();
    let out = app.session.acknowledge_window(&pending.request_id, true);
    app.messages(out);
    app
}

fn local_hidden(app: &mut BoardApp) {
    let pending = app.session.pending_window_request().unwrap().clone();
    assert!(!pending.visible);
    let out = app.session.acknowledge_window(&pending.request_id, false);
    app.messages(out);
}

#[test]
fn local_capture_visible_option_has_no_hide_lease_and_keeps_snapshot_choice() {
    let mut app = local_capture_app();
    assert!(app.capture_hide_window);
    app.capture_hide_window = false;
    let before = app.session.state();
    app.begin_local_capture();
    assert_eq!(app.session.state(), before);
    assert!(app.session.pending_window_request().is_none());
    app.capture_hide_window = true;
    app.poll_local_capture();
    assert_eq!(app.local_capture_test_state().0, "armed");
    app.finish_local_capture(Ok(capture_png()));
    assert_eq!(app.session.state()["hide_lease_count"], 0);
    assert!(app.session.effective_visible());
    assert_eq!(app.session.document.current_page().objects.len(), 1);
    app.undo(false);
    assert!(app.session.document.current_page().objects.is_empty());
}

#[test]
fn visible_capture_failure_and_external_hide_are_not_overridden() {
    for failure in [false, true] {
        let mut app = local_capture_app();
        app.capture_hide_window = false;
        app.begin_local_capture();
        let out = app
            .session
            .handle(Request::new("neo:hide", "hide", serde_json::json!({})).unwrap());
        app.messages(out);
        if failure {
            app.finish_local_capture(Err("模拟取消".into()));
            assert!(app.local_capture.is_none());
        } else {
            app.finish_local_capture(Ok(capture_png()));
        }
        assert_eq!(app.session.state()["desired_visible"], false);
        assert!(!app.session.effective_visible());
        assert_eq!(app.session.state()["hide_lease_count"], 0);
    }
}

#[test]
fn local_capture_hide_checkbox_is_clickable_before_start_and_disabled_for_hosted() {
    let mut app = local_capture_app();
    let ctx = egui::Context::default();
    open_toolbar_menu(&mut app, &ctx, "数学 / 模型 / 截图 / AI");
    click_text(&mut app, &ctx, "截图时隐藏窗口");
    assert!(!app.capture_hide_window);
    click_text(&mut app, &ctx, "截图（本次授权）");
    assert_eq!(app.session.state()["hide_lease_count"], 0);
    assert!(app.session.effective_visible());
    assert!(app.local_capture.is_some());

    let mut hosted = local_capture_app();
    hosted.hosted = true;
    let ctx = egui::Context::default();
    open_toolbar_menu(&mut hosted, &ctx, "数学 / 模型 / 截图 / AI");
    click_text(&mut hosted, &ctx, "截图时隐藏窗口");
    assert!(hosted.capture_hide_window);
    assert!(hosted.local_capture.is_none());
}

#[test]
fn local_capture_waits_for_native_hide_and_an_extra_logic_round_single_slot() {
    let mut app = local_capture_app();
    let permissions = app.session.state()["permissions"].clone();
    app.begin_local_capture();
    app.begin_local_capture();
    assert_eq!(app.session.state()["hide_lease_count"], 1);
    app.poll_local_capture();
    assert_eq!(app.local_capture_test_state().0, "waiting");
    local_hidden(&mut app);
    app.poll_local_capture();
    assert_eq!(app.local_capture_test_state().0, "armed");
    assert_eq!(app.session.state()["permissions"], permissions);
    // The next logic round alone may launch the worker. Even if accidentally polled,
    // crate::local_capture::capture is disabled in test builds.
    app.poll_local_capture();
    assert_eq!(app.local_capture_test_state().0, "running");
    assert!(!app.emitted.iter().any(|m| matches!(m, Message::Request(_))));
}

#[test]
fn local_capture_rejects_untracked_windows_and_resets_hidden_barrier() {
    let mut app = local_capture_app();
    app.begin_local_capture();
    local_hidden(&mut app);
    app.poll_local_capture();
    assert_eq!(app.local_capture_test_state().0, "armed");
    app.session.set_owned_windows(&["main", "other"]).unwrap();
    let request = app.session.pending_window_request().unwrap().clone();
    app.session
        .acknowledge_windows(&request.request_id, &[("main", false), ("other", false)]);
    app.poll_local_capture();
    assert_eq!(app.local_capture_test_state().0, "waiting");
    let mut other = local_capture_app();
    other.session.set_owned_windows(&["main", "other"]).unwrap();
    other.begin_local_capture();
    assert!(other.local_capture.is_none());
}

#[test]
fn local_capture_fake_success_imports_once_one_undo_and_preserves_visibility_intent() {
    for hide in [false, true] {
        let mut app = local_capture_app();
        let permissions = app.session.state()["permissions"].clone();
        app.canvas_size = Vec2::new(180.0, 180.0);
        app.begin_local_capture();
        local_hidden(&mut app);
        if hide {
            app.incoming_message(
                Request::new("neo:hide", "hide", serde_json::json!({}))
                    .unwrap()
                    .into(),
            );
        }
        app.finish_local_capture(Ok(capture_png()));
        app.finish_local_capture(Ok(capture_png()));
        assert_eq!(app.session.document.revision, 1);
        assert_eq!(app.session.document.current_page().objects.len(), 1);
        assert_eq!(app.session.state()["hide_lease_count"], 0);
        assert_eq!(app.session.state()["desired_visible"], !hide);
        assert_eq!(app.session.state()["effective_visible"], !hide);
        assert_eq!(app.session.state()["permissions"], permissions);
        assert!(app.local_capture.is_none());
        assert!(!app.emitted.iter().any(|m| matches!(m, Message::Request(_))));
        app.undo(false);
        assert!(app.session.document.current_page().objects.is_empty());
        app.undo(true);
        assert_eq!(app.session.document.current_page().objects.len(), 1);
    }
}

#[test]
fn local_capture_fake_stale_results_discard_before_import() {
    for reason in ["revision", "page", "document", "roundtrip"] {
        let mut app = local_capture_app();
        app.session.document.add_page().unwrap();
        app.session.document.set_current_page(0).unwrap();
        app.session.history = board_core::History::new(&app.session.document);
        app.begin_local_capture();
        local_hidden(&mut app);
        match reason {
            "revision" => app.add(text_kind("new revision")),
            "document" => app.incoming_message(
                Request::new(
                    "neo:new",
                    "document.new",
                    serde_json::json!({"discard_unsaved":true}),
                )
                .unwrap()
                .into(),
            ),
            _ => {
                app.session.document.set_current_page(1).unwrap();
                app.invalidate_captures();
                if reason == "roundtrip" {
                    app.session.document.set_current_page(0).unwrap();
                }
            }
        }
        let before = app.session.document.clone();
        app.finish_local_capture(Ok(capture_png()));
        assert_eq!(app.session.document, before);
        assert!(app.captured_asset.is_none(), "{reason}: {}", app.status);
        assert!(app.status.contains("未导入"), "{reason}: {}", app.status);
        assert_eq!(app.session.state()["hide_lease_count"], 0);
    }
}

#[test]
fn local_capture_failure_and_cancel_restore_only_after_worker_completion() {
    for cancelled in [false, true] {
        let mut app = local_capture_app();
        app.begin_local_capture();
        local_hidden(&mut app);
        if cancelled {
            app.cancel_local_capture();
            assert_eq!(app.session.state()["hide_lease_count"], 1);
            assert!(!app.session.effective_visible());
        }
        app.finish_local_capture(if cancelled {
            Ok(capture_png())
        } else {
            Err("模拟内置框选超时".into())
        });
        assert!(app.local_capture.is_none());
        assert!(app.session.effective_visible());
        assert_eq!(app.session.state()["hide_lease_count"], 0);
        assert!(app.session.document.current_page().objects.is_empty());
        assert!(!app.allow_close);
        assert_eq!(app.session.state()["owned_window_count"], 1);
    }
}

#[test]
fn local_capture_drop_and_close_signal_worker_cancellation() {
    use std::sync::atomic::Ordering;
    let mut app = local_capture_app();
    app.begin_local_capture();
    let cancel = app.local_capture_test_state().1;
    drop(app);
    assert!(cancel.load(Ordering::Acquire));
    let mut app = local_capture_app();
    app.begin_local_capture();
    let cancel = app.local_capture_test_state().1;
    app.incoming_message(
        Request::new("neo:close", "close", serde_json::json!({}))
            .unwrap()
            .into(),
    );
    assert!(cancel.load(Ordering::Acquire));
    app.finish_local_capture(Ok(capture_png()));
    assert!(app.session.document.current_page().objects.is_empty());
}

#[test]
fn screenshot_click_routes_hosted_directly_without_changing_permissions() {
    for allowed in [false, true] {
        let mut app = if allowed {
            capture_app()
        } else {
            local_capture_app()
        };
        app.hosted = true;
        let permissions = app.session.state()["permissions"].clone();
        app.screenshot_clicked();
        // No host request can be emitted until the native hide ack. The denied case
        // never hides. Avoid host output in this memory-only test after this point.
        app.hosted = false;
        assert!(app.authorization.is_none());
        assert!(app.local_capture.is_none());
        assert_eq!(app.session.state()["permissions"], permissions);
        assert_eq!(app.jobs.len(), usize::from(allowed));
    }
}

#[test]
fn local_capture_invalid_png_restores_without_inserting() {
    let mut app = local_capture_app();
    app.begin_local_capture();
    local_hidden(&mut app);
    app.finish_local_capture(Ok(vec![1, 2, 3]));
    assert!(app.local_capture.is_none());
    assert!(app.session.effective_visible());
    assert_eq!(app.session.document.revision, 0);
    assert!(app.captured_asset.is_none());
}

#[test]
fn local_capture_recovery_and_success_release_only_their_own_lease() {
    for failure in [false, true] {
        let mut app = local_capture_app();
        app.begin_local_capture();
        local_hidden(&mut app);
        app.incoming_message(
            Request::new(
                "neo:other",
                "window.suspend",
                serde_json::json!({"lease_id":"other"}),
            )
            .unwrap()
            .into(),
        );
        if failure {
            app.finish_local_capture(Err("cancel/timeout".into()));
        } else {
            app.finish_local_capture(Ok(capture_png()));
        }
        assert_eq!(app.session.state()["hide_lease_count"], 1);
        assert_eq!(app.session.state()["desired_visible"], true);
        assert!(!app.session.effective_visible());
    }
}

#[test]
fn screenshot_menu_pointer_click_is_the_only_local_authorization_step() {
    if !cfg!(windows) {
        return;
    }
    let mut app = local_capture_app();
    let ctx = egui::Context::default();
    let permissions = app.session.state()["permissions"].clone();
    open_toolbar_menu(&mut app, &ctx, "数学 / 模型 / 截图 / AI");
    click_text(&mut app, &ctx, "截图（本次授权）");
    assert!(app.local_capture.is_some());
    assert!(app.authorization.is_none());
    assert_eq!(app.session.state()["permissions"], permissions);
    assert_eq!(app.local_capture_test_state().0, "waiting");
}

fn click_image_frame(app: &mut BoardApp, ctx: &egui::Context, pos: Pos2) {
    frame(app, ctx, Vec2::new(800.0, 600.0), vec![], false);
    frame(
        app,
        ctx,
        Vec2::new(800.0, 600.0),
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
        false,
    );
    release(app, ctx, pos);
}

fn image_click_app() -> (BoardApp, egui::Context) {
    let mut app = app();
    app.tool = Tool::Select;
    for (id, x) in [("image-a", 100.0), ("image-b", 400.0)] {
        app.session.document.pages[0].objects.push(BoardObject {
            id: id.into(),
            kind: ObjectKind::Image {
                position: Point { x, y: 100.0 },
                width: 200.0,
                height: 160.0,
                asset_ref: format!("asset:{id}"),
            },
        });
    }
    app.session.history = board_core::History::new(&app.session.document);
    (app, egui::Context::default())
}

#[test]
fn capture_success_selects_image_and_next_pointer_click_opens_opt_in_agent() {
    for source in ["local", "host-response", "host-event"] {
        let mut app = if source == "local" {
            local_capture_app()
        } else {
            capture_app()
        };
        app.canvas_size = Vec2::new(800.0, 600.0);
        assert!(app.tool == Tool::Pen);
        if source == "local" {
            app.begin_local_capture();
            local_hidden(&mut app);
            app.finish_local_capture(Ok(capture_png()));
        } else {
            let (_, host) = start_capture(&mut app);
            if source == "host-event" {
                app.incoming_message(
                    board_protocol::Response::success(
                        &host.id,
                        serde_json::json!({"job_id": "host-selected-image"}),
                    )
                    .unwrap()
                    .into(),
                );
                app.incoming_message(
                    Event::new(
                        "job.finished",
                        serde_json::json!({
                            "job_id": "host-selected-image", "ok": true,
                            "result": {"png_bytes": capture_png()}
                        }),
                    )
                    .into(),
                );
            } else {
                app.incoming_message(
                    board_protocol::Response::success(
                        &host.id,
                        serde_json::json!({"png_bytes": capture_png()}),
                    )
                    .unwrap()
                    .into(),
                );
            }
        }
        let image = app.session.document.current_page().objects[0].clone();
        let ObjectKind::Image { asset_ref, .. } = &image.kind else {
            panic!("image expected");
        };
        assert!(app.tool == Tool::Select, "{source}");
        assert_eq!(app.selected.as_deref(), Some(image.id.as_str()), "{source}");
        assert!(app.authorization.is_none());
        assert!(!app.authorize_assets && !app.authorize_write_back);
        assert!(app.jobs.is_empty());
        assert!(
            !app.emitted
                .iter()
                .any(|m| matches!(m, Message::Request(r) if r.method == "host.ask_agent"))
        );
        let pending = app.session.pending_window_request().unwrap().clone();
        let out = app.session.acknowledge_window(&pending.request_id, true);
        app.messages(out);
        app.emitted.clear();
        let before = app.session.document.clone();
        let ctx = egui::Context::default();
        // Full board UI includes the image's separate button and all overlays. Click
        // its center, not the button; no tool switch is made after capture completion.
        board_frame(&mut app, &ctx, vec![]);
        board_frame(&mut app, &ctx, vec![]);
        let pos = board_render::object_bounds(&image).center();
        board_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
        );
        board_frame(&mut app, &ctx, vec![pointer(pos, false)]);
        assert_eq!(app.authorization, Some(false), "{source}");
        let (_, id, asset) = app.agent_image.as_ref().unwrap();
        assert_eq!(id, &image.id);
        assert_eq!(asset, asset_ref);
        assert!(!app.authorize_assets && !app.authorize_write_back);
        assert!(app.authorized_asset_refs().is_empty());
        assert!(app.jobs.is_empty());
        assert!(
            app.emitted
                .iter()
                .all(|m| !matches!(m, Message::Request(_)))
        );
        assert_eq!(app.session.document, before);

        app.cancel_authorization();
        let end = pos + Vec2::new(30.0, 20.0);
        drag_to(&mut app, &ctx, pos, end);
        release(&mut app, &ctx, end);
        assert!(app.authorization.is_none());
        assert_eq!(app.session.document.revision, before.revision + 1);
        app.tool = Tool::Pen;
        drag_to(&mut app, &ctx, end, end + Vec2::new(20.0, 0.0));
        release(&mut app, &ctx, end + Vec2::new(20.0, 0.0));
        assert!(app.authorization.is_none());
        assert!(matches!(
            app.session
                .document
                .current_page()
                .objects
                .last()
                .unwrap()
                .kind,
            ObjectKind::Stroke { .. }
        ));
    }
}

#[test]
fn image_single_click_opens_only_same_image_authorization_without_sending() {
    let (mut app, ctx) = image_click_app();
    click_image_frame(&mut app, &ctx, Pos2::new(180.0, 180.0));
    assert_eq!(app.agent_image.as_ref().unwrap().1, "image-a");
    assert_eq!(app.authorization, Some(false));
    assert!(!app.authorize_assets && !app.authorize_write_back);
    assert_eq!(app.session.document.revision, 0);
    assert!(app.jobs.is_empty());
    assert!(app.tool == Tool::Select);
    app.cancel_authorization();
    app.authorize_assets = true;
    app.authorize_write_back = true;
    click_image_frame(&mut app, &ctx, Pos2::new(480.0, 180.0));
    assert_eq!(app.agent_image.as_ref().unwrap().1, "image-b");
    assert!(!app.authorize_assets && !app.authorize_write_back);
    app.authorize_assets = true;
    assert_eq!(app.authorized_asset_refs(), vec!["asset:image-b"]);
    assert!(
        app.emitted
            .iter()
            .all(|m| !matches!(m, Message::Request(_)))
    );
}

#[test]
fn image_drag_pen_eraser_overlay_and_top_object_do_not_open_agent() {
    for action in ["drag", "pen", "eraser", "overlay", "top-object"] {
        let (mut app, ctx) = image_click_app();
        let start = Pos2::new(180.0, 180.0);
        match action {
            "pen" => app.tool = Tool::Pen,
            "eraser" => app.tool = Tool::Eraser,
            "overlay" => app.controls.push(Rect::from_min_max(
                Pos2::new(150.0, 150.0),
                Pos2::new(230.0, 230.0),
            )),
            "top-object" => app.session.document.pages[0].objects.push(edit_shape(
                ShapeKind::Line,
                Point { x: 170.0, y: 170.0 },
                Point { x: 190.0, y: 190.0 },
            )),
            _ => {}
        }
        if ["drag", "pen", "eraser"].contains(&action) {
            let end = start + Vec2::new(40.0, 20.0);
            drag_to(&mut app, &ctx, start, end);
            release(&mut app, &ctx, end);
            assert!(app.session.document.revision > 0, "{action}");
        } else {
            click_image_frame(&mut app, &ctx, start);
        }
        assert!(app.authorization.is_none(), "{action}");
        assert!(app.agent_image.is_none(), "{action}");
    }
}
