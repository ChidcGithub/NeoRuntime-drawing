fn resize_test_object(image: bool) -> BoardObject {
    let mut object = drag_plot("resizable", 200.0, &["x", "y^2=x"]);
    if image {
        object.kind = ObjectKind::Image {
            position: Point { x: 200.0, y: 100.0 },
            width: 200.0,
            height: 100.0,
            asset_ref: "asset:synthetic".into(),
        };
    } else if let ObjectKind::FunctionPlot { width, .. } = &mut object.kind {
        *width = 200.0;
    }
    object
}

fn resize_test_app(mode: AppMode, image: bool) -> (BoardApp, egui::Context, BoardObject) {
    let object = resize_test_object(image);
    let mut app = BoardApp::new(
        mode,
        Session::new(mode.kind()),
        false,
        None,
        Default::default(),
    );
    app.tool = Tool::Select;
    app.selected = Some(object.id.clone());
    app.session.document.pages[0].objects = vec![object.clone()];
    app.session.history = board_core::History::new(&app.session.document);
    (app, egui::Context::default(), object)
}

#[test]
fn frame_resize_pointer_all_handles_both_modes_preview_and_single_history() {
    for mode in [AppMode::Drawing, AppMode::Blackboard] {
        for image in [false, true] {
            for index in 0..4 {
                for amount in [-40.0, 60.0] {
                    let (mut app, ctx, original) = resize_test_app(mode, image);
                    let handles = editing::resize_handles(&original);
                    let anchor = handles[(index + 2) % 4];
                    let start = Pos2::new(handles[index].x, handles[index].y);
                    let end = start
                        + Vec2::new(
                            amount * if index == 0 || index == 3 { -1.0 } else { 1.0 },
                            amount * if index < 2 { -1.0 } else { 1.0 },
                        );
                    let output = drag_to(&mut app, &ctx, start, end);
                    assert_eq!(app.gesture.as_ref().unwrap().resize, Some(index));
                    assert_eq!(app.gesture.as_ref().unwrap().vertex, None);
                    let preview = app.preview().unwrap();
                    let resized = editing::resize_handles(&preview);
                    assert_eq!(resized[(index + 2) % 4], anchor);
                    let (width, height) =
                        (resized[2].x - resized[0].x, resized[2].y - resized[0].y);
                    if image {
                        assert!((width / height - 2.0).abs() < 0.0001);
                        assert!((width - (200.0 + amount * 1.2)).abs() < 0.001);
                    } else {
                        assert_eq!((width, height), (200.0 + amount, 100.0 + amount));
                        let ObjectKind::FunctionPlot {
                            expressions,
                            x_min,
                            x_max,
                            y_min,
                            y_max,
                            ..
                        } = &preview.kind
                        else {
                            panic!()
                        };
                        assert_eq!(expressions, &["x", "y^2=x"]);
                        assert_eq!((*x_min, *x_max, *y_min, *y_max), (-10.0, 10.0, -7.0, 7.0));
                    }
                    for handle in resized {
                        assert!(output.shapes.iter().any(|s| matches!(&s.shape,
                            egui::Shape::Rect(r) if r.fill == Color32::LIGHT_BLUE
                                && r.rect.size() == Vec2::splat(10.0)
                                && r.rect.center() == Pos2::new(handle.x, handle.y))));
                    }
                    assert_eq!(
                        app.session.document.current_page().objects,
                        vec![original.clone()]
                    );
                    assert_eq!(app.session.document.revision, 0);
                    assert!(!app.session.history.can_undo());
                    let samples = app.renderer.plot_sample_calls();
                    release(&mut app, &ctx, end);
                    assert_eq!(
                        app.session.document.current_page().objects,
                        vec![preview.clone()]
                    );
                    assert_eq!(app.session.document.revision, 1);
                    assert!(app.authorization.is_none());
                    assert!(app.agent_image.is_none());
                    app.undo(false);
                    assert_eq!(app.session.document.current_page().objects, vec![original]);
                    assert!(!app.session.history.can_undo());
                    app.undo(true);
                    assert_eq!(app.session.document.current_page().objects, vec![preview]);
                    assert!(!app.session.history.can_redo());
                    frame(&mut app, &ctx, Vec2::new(800.0, 600.0), vec![], false);
                    assert_eq!(app.renderer.plot_sample_calls(), samples);
                }
            }
        }
    }
}

#[test]
fn frame_resize_stationary_return_to_start_and_stale_never_commit_or_authorize() {
    for mode in [AppMode::Drawing, AppMode::Blackboard] {
        for image in [false, true] {
            for index in 0..4 {
                for return_to_start in [false, true] {
                    let (mut app, ctx, original) = resize_test_app(mode, image);
                    let p = editing::resize_handles(&original)[index];
                    // Hit-area offset must not resize a stationary click.
                    let start = Pos2::new(p.x + 2.0, p.y + 2.0);
                    let end = start
                        + if return_to_start {
                            Vec2::splat(30.0)
                        } else {
                            Vec2::ZERO
                        };
                    drag_to(&mut app, &ctx, start, end);
                    frame(
                        &mut app,
                        &ctx,
                        Vec2::new(800.0, 600.0),
                        vec![egui::Event::PointerMoved(start), pointer(start, false)],
                        false,
                    );
                    assert_eq!(app.session.document.current_page().objects, vec![original]);
                    assert_eq!(app.session.document.revision, 0);
                    assert!(!app.session.history.can_undo());
                    assert!(app.authorization.is_none());
                    assert!(app.gesture.is_none());
                }
            }
            for stale in 0..3 {
                let (mut app, ctx, _) = resize_test_app(mode, image);
                let end = Pos2::new(450.0, 260.0);
                drag_to(&mut app, &ctx, Pos2::new(400.0, 200.0), end);
                match stale {
                    0 => app.session.document.id = new_id(),
                    1 => {
                        app.session.document.add_page().unwrap();
                    }
                    _ => app.session.document.revision += 1,
                }
                let before = app.session.document.clone();
                assert!(app.preview().is_none());
                release(&mut app, &ctx, end);
                assert_eq!(app.session.document, before);
                assert!(!app.session.history.can_undo());
                assert!(app.authorization.is_none());
                assert!(app.gesture.is_none());
            }
        }
    }
}

#[test]
fn frame_resize_overlap_is_not_plot_merge() {
    for mode in [AppMode::Drawing, AppMode::Blackboard] {
        let (mut app, ctx, _) = resize_test_app(mode, false);
        let target = drag_plot("target", 430.0, &["x^2"]);
        app.session.document.pages[0].objects.push(target.clone());
        app.session.history = board_core::History::new(&app.session.document);
        let end = Pos2::new(480.0, 250.0);
        let output = drag_to(&mut app, &ctx, Pos2::new(400.0, 200.0), end);
        assert!(merge_highlights(&output).is_empty());
        let preview = app.preview().unwrap();
        assert!(app.plot_merge_target(&preview).is_none());
        let output = frame(
            &mut app,
            &ctx,
            Vec2::new(800.0, 600.0),
            vec![pointer(end, false)],
            false,
        );
        assert!(merge_highlights(&output).is_empty());
        assert_eq!(
            app.session.document.current_page().objects,
            vec![preview, target]
        );
        assert_eq!(app.session.document.revision, 1);
    }
}

#[test]
fn frame_resize_pointer_axis_minimum_and_wheel_range_zoom_stay_separate() {
    for mode in [AppMode::Drawing, AppMode::Blackboard] {
        for image in [false, true] {
            for delta in [
                Vec2::new(60.0, 0.0),
                Vec2::new(0.0, -40.0),
                Vec2::new(-250.0, -150.0),
            ] {
                let (mut app, ctx, _) = resize_test_app(mode, image);
                let start = Pos2::new(400.0, 200.0);
                let end = start + delta;
                drag_to(&mut app, &ctx, start, end);
                let preview = app.preview().unwrap();
                let handles = editing::resize_handles(&preview);
                let (w, h) = (handles[2].x - handles[0].x, handles[2].y - handles[0].y);
                assert!(w >= 32.0 && h >= 32.0);
                assert_eq!(handles[0], Point { x: 200.0, y: 100.0 });
                if image {
                    assert!((w / h - 2.0).abs() < 0.0001);
                } else {
                    assert_eq!(
                        (w, h),
                        ((200.0 + delta.x).max(32.0), (100.0 + delta.y).max(32.0))
                    );
                }
                release(&mut app, &ctx, end);
                assert_eq!(app.session.document.current_page().objects, vec![preview]);
                assert_eq!(app.session.document.revision, 1);
                assert!(app.authorization.is_none());
            }
        }
        let (mut app, ctx, original) = resize_test_app(mode, false);
        let mut expected = original.clone();
        editing::scale_plot(&mut expected, 0.9);
        frame(&mut app, &ctx, Vec2::new(800.0, 600.0), vec![], false);
        frame(
            &mut app,
            &ctx,
            Vec2::new(800.0, 600.0),
            vec![
                egui::Event::PointerMoved(Pos2::new(300.0, 150.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, 30.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: Default::default(),
                },
            ],
            false,
        );
        assert_eq!(
            app.session.document.current_page().objects,
            vec![expected.clone()]
        );
        assert_eq!(
            editing::resize_handles(&expected),
            editing::resize_handles(&original)
        );
        assert_ne!(expected, original);
        app.undo(false);
        assert_eq!(app.session.document.current_page().objects, vec![original]);
        assert!(!app.session.history.can_undo());
    }
}

fn frame_resize_overlay_frame(app: &mut BoardApp, ctx: &egui::Context, events: Vec<egui::Event>) {
    app.controls.clear();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
            events,
            ..Default::default()
        },
        |ui| {
            app.plot_controls(ctx);
            app.canvas(ui);
        },
    );
    output.textures_delta.clear();
}

#[test]
fn frame_resize_image_overlay_keeps_handles_center_move_and_agent_button_distinct() {
    for mode in [AppMode::Drawing, AppMode::Blackboard] {
        for index in 0..4 {
            let (mut app, ctx, original) = resize_test_app(mode, true);
            frame_resize_overlay_frame(&mut app, &ctx, vec![]);
            frame_resize_overlay_frame(&mut app, &ctx, vec![]);
            let p = editing::resize_handles(&original)[index];
            let start = Pos2::new(p.x, p.y);
            assert!(!app.controls.iter().any(|rect| rect.contains(start)));
            frame_resize_overlay_frame(
                &mut app,
                &ctx,
                vec![egui::Event::PointerMoved(start), pointer(start, true)],
            );
            assert_eq!(app.gesture.as_ref().unwrap().resize, Some(index));
            frame_resize_overlay_frame(&mut app, &ctx, vec![pointer(start, false)]);
            assert!(app.authorization.is_none());
            assert_eq!(app.session.document.revision, 0);
        }
        let (mut app, ctx, original) = resize_test_app(mode, true);
        let start = Pos2::new(300.0, 150.0);
        let end = start + Vec2::new(30.0, 40.0);
        frame_resize_overlay_frame(&mut app, &ctx, vec![]);
        frame_resize_overlay_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(start), pointer(start, true)],
        );
        assert_eq!(app.gesture.as_ref().unwrap().resize, None);
        frame_resize_overlay_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
        frame_resize_overlay_frame(&mut app, &ctx, vec![pointer(end, false)]);
        let mut moved = original;
        editing::translate(&mut moved, Point { x: 30.0, y: 40.0 });
        assert_eq!(app.session.document.current_page().objects, vec![moved]);
        assert!(app.authorization.is_none());
        frame_resize_overlay_frame(&mut app, &ctx, vec![pointer(end, true)]);
        frame_resize_overlay_frame(&mut app, &ctx, vec![pointer(end, false)]);
        assert_eq!(app.authorization, Some(false));
        assert!(!app.authorize_assets);
        assert!(!app.authorize_write_back);
        assert_eq!(app.session.document.revision, 1);

        let (mut app, ctx, original) = resize_test_app(mode, true);
        app.selected = None;
        frame_resize_overlay_frame(&mut app, &ctx, vec![]);
        frame_resize_overlay_frame(&mut app, &ctx, vec![]);
        let button = features::image_agent_rect(&original, ctx.content_rect())
            .unwrap()
            .center();
        assert!(app.controls.iter().any(|r| r.contains(button)));
        frame_resize_overlay_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(button), pointer(button, true)],
        );
        assert!(app.gesture.is_none());
        frame_resize_overlay_frame(&mut app, &ctx, vec![pointer(button, false)]);
        assert_eq!(app.authorization, Some(false));
        assert!(!app.authorize_assets);
        assert!(!app.authorize_write_back);
        assert_eq!(app.session.document.revision, 0);
    }
}
