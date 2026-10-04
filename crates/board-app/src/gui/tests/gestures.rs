#[test]
fn held_pointer_does_not_restart_after_remote_page_change() {
    for tool in [Tool::Pen, Tool::Eraser, Tool::Select, Tool::Shape] {
        let mut app = app();
        let ctx = egui::Context::default();
        let size = Vec2::new(800.0, 600.0);
        app.tool = tool;
        app.session.document.add_page().unwrap();
        app.session.document.set_current_page(0).unwrap();
        let next = app.session.document.pages[1].id.clone();
        frame(&mut app, &ctx, size, vec![], false);
        let pos = Pos2::new(100.0, 100.0);
        frame(
            &mut app,
            &ctx,
            size,
            vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
            false,
        );
        assert!(app.gesture.is_some());
        app.incoming_message(
            Request::new(
                "neo:page",
                "pages.select",
                serde_json::json!({"document_id": app.session.document.id, "page_id": next}),
            )
            .unwrap()
            .into(),
        );
        assert!(app.gesture.is_none());
        let moved = Pos2::new(160.0, 150.0);
        frame(
            &mut app,
            &ctx,
            size,
            vec![egui::Event::PointerMoved(moved)],
            false,
        );
        assert!(app.gesture.is_none());
        frame(&mut app, &ctx, size, vec![pointer(moved, false)], false);
        assert!(
            app.session
                .document
                .pages
                .iter()
                .all(|page| page.objects.is_empty())
        );
    }
}

#[test]
fn host_writeback_response_and_event_cancel_gesture_and_split_drag() {
    for asynchronous in [false, true] {
        let mut app = app();
        app.session.handle(Request::new("neo:allow", "configure", serde_json::json!({"classroom_safe": true, "desktop_capture_allowed": false, "agent_allowed": true})).unwrap());
        app.authorize_write_back = true;
        app.agent_prompt = "test".into();
        let gui_request = app.take_host_request(false).unwrap();
        assert!(!app.authorize_write_back);
        let out = app.session.handle(gui_request);
        let ctx = egui::Context::default();
        app.textures.insert(
            "asset:cached".into(),
            ctx.load_texture(
                "cached",
                egui::ColorImage::new([1, 1], vec![Color32::RED]),
                Default::default(),
            ),
        );
        app.plot_points = Some(PlotState {
            context: ContextToken::capture(&app.session.document),
            object: BoardObject {
                id: "plot".into(),
                kind: text_kind("f(x)=x"),
            },
            report: Some(Ok(Default::default())),
            coordinate: None,
        });
        let request = out
            .iter()
            .find_map(|message| match message {
                Message::Request(request) if request.method == "host.ask_agent" => {
                    Some(request.clone())
                }
                _ => None,
            })
            .unwrap();
        let context = ContextToken::capture(&app.session.document);
        app.gesture = Some(Gesture {
            document: context.document_id.clone(),
            page: context.page_id.clone(),
            revision: context.revision,
            points: vec![StrokePoint {
                x: 10.0,
                y: 10.0,
                time: 0.0,
                pressure: 1.0,
            }],
            original: None,
            vertex: None,
            resize: None,
            erasing: None,
        });
        app.split_drag = Some(("plot".into(), 0, context));
        let result = serde_json::json!({"answer": "ok", "operations": [{"op": "add", "object": {"id": "host-text", "kind": {"type": "text", "position": {"x": 40.0, "y": 40.0}, "text": "result", "size": 24.0, "color": {"r": 0, "g": 0, "b": 0, "a": 255}}}}]});
        if asynchronous {
            app.incoming_message(
                board_protocol::Response::success(
                    &request.id,
                    serde_json::json!({"job_id": "host-job"}),
                )
                .unwrap()
                .into(),
            );
            assert!(app.gesture.is_some());
            app.incoming_message(
                Event::new(
                    "job.finished",
                    serde_json::json!({"job_id": "host-job", "ok": true, "result": result}),
                )
                .into(),
            );
        } else {
            app.incoming_message(
                board_protocol::Response::success(&request.id, result)
                    .unwrap()
                    .into(),
            );
        }
        assert!(app.gesture.is_none());
        assert!(app.split_drag.is_none());
        assert!(app.textures.is_empty());
        assert!(app.plot_points.is_none());
        assert_eq!(app.session.document.revision, 1);
        assert_eq!(app.session.document.current_page().objects.len(), 1);
        assert_eq!(
            app.session.document.current_page().objects[0].id,
            "host-text"
        );
    }
}

#[test]
fn ordinary_pen_gesture_still_commits_once() {
    let mut app = app();
    let ctx = egui::Context::default();
    let size = Vec2::new(800.0, 600.0);
    frame(&mut app, &ctx, size, vec![], false);
    let pos = Pos2::new(100.0, 100.0);
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
        false,
    );
    let end = Pos2::new(150.0, 150.0);
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(end)],
        false,
    );
    frame(&mut app, &ctx, size, vec![pointer(end, false)], false);
    assert_eq!(app.session.document.current_page().objects.len(), 1);
    assert!(app.session.history.can_undo());
}

#[test]
fn line_endpoint_click_without_drag_preserves_geometry_and_history() {
    for pos in [Pos2::new(105.0, 104.0), Pos2::new(295.0, 204.0)] {
        let mut app = app();
        app.tool = Tool::Select;
        let line = edit_shape(
            ShapeKind::Line,
            Point { x: 100.0, y: 100.0 },
            Point { x: 300.0, y: 200.0 },
        );
        app.selected = Some(line.id.clone());
        app.session.document.pages[0].objects = vec![line.clone()];
        app.session.history = board_core::History::new(&app.session.document);
        let before = app.session.document.clone();
        let ctx = egui::Context::default();
        let size = Vec2::new(800.0, 600.0);
        frame(&mut app, &ctx, size, vec![], false);
        frame(
            &mut app,
            &ctx,
            size,
            vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
            false,
        );
        assert!(app.gesture.as_ref().unwrap().vertex.is_some());
        let preview = app.preview().unwrap();
        release(&mut app, &ctx, pos);
        assert_eq!(app.session.document, before, "endpoint click must not edit");
        assert_eq!(
            preview, line,
            "stationary preview must not move the endpoint"
        );
        assert!(!app.session.history.can_undo());
        assert!(!app.session.history.is_dirty(&app.session.document));
    }
}

#[test]
fn line_endpoint_drag_still_commits_with_single_undo_redo() {
    let mut line = edit_shape(
        ShapeKind::Line,
        Point { x: 100.0, y: 100.0 },
        Point { x: 300.0, y: 200.0 },
    );
    line.id = "line".into();
    let (mut app, ctx) = plot_drag_app(vec![line.clone()]);
    app.selected = Some(line.id.clone());
    let end = Pos2::new(140.0, 150.0);
    drag_to(&mut app, &ctx, Pos2::new(105.0, 104.0), end);
    let preview = app.preview().unwrap();
    assert_ne!(preview, line);
    assert_eq!(app.session.document.current_page().objects, vec![line.clone()]);
    release(&mut app, &ctx, end);
    assert_eq!(app.session.document.current_page().objects, vec![preview.clone()]);
    assert_eq!(app.session.document.revision, 1);
    app.undo(false);
    assert_eq!(app.session.document.current_page().objects, vec![line]);
    assert!(!app.session.history.can_undo());
    assert!(!app.session.history.is_dirty(&app.session.document));
    app.undo(true);
    assert_eq!(app.session.document.current_page().objects, vec![preview]);
    assert!(!app.session.history.can_redo());
}

#[test]
fn single_frame_tap_creates_a_dot() {
    let mut app = app();
    let ctx = egui::Context::default();
    let size = Vec2::new(800.0, 600.0);
    frame(&mut app, &ctx, size, vec![], false);
    let pos = Pos2::new(100.0, 100.0);
    frame(
        &mut app,
        &ctx,
        size,
        vec![
            egui::Event::PointerMoved(pos),
            pointer(pos, true),
            pointer(pos, false),
        ],
        false,
    );
    assert_eq!(app.session.document.current_page().objects.len(), 1);
}
