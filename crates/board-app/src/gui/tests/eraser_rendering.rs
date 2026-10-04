fn eraser_app() -> (BoardApp, egui::Context) {
    let mut app = app();
    app.tool = Tool::Eraser;
    app.session.document.pages[0].objects = vec![BoardObject {
        id: "eraser-stroke".into(),
        kind: ObjectKind::Stroke {
            points: vec![
                StrokePoint {
                    x: 100.0,
                    y: 150.0,
                    time: 0.0,
                    pressure: 1.0,
                },
                StrokePoint {
                    x: 300.0,
                    y: 150.0,
                    time: 1.0,
                    pressure: 1.0,
                },
            ],
            style: Style::default(),
        },
    }];
    app.session.history = board_core::History::new(&app.session.document);
    (app, egui::Context::default())
}

fn eraser_frame(
    app: &mut BoardApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    frame(app, ctx, Vec2::new(800.0, 600.0), events, false)
}

fn erased_document(app: &BoardApp) -> &board_core::Document {
    &app.gesture
        .as_ref()
        .unwrap()
        .erasing
        .as_ref()
        .unwrap()
        .result
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap()
        .0
}

fn stroke_length(document: &board_core::Document) -> f32 {
    document
        .current_page()
        .objects
        .iter()
        .map(|object| {
            if let ObjectKind::Stroke { points, .. } = &object.kind {
                points
                    .windows(2)
                    .map(|pair| (pair[1].x - pair[0].x).hypot(pair[1].y - pair[0].y))
                    .sum()
            } else {
                0.0
            }
        })
        .sum()
}

fn ink_mesh(output: &egui::FullOutput) -> std::sync::Arc<egui::Mesh> {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Mesh(mesh) => Some(mesh.clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn eraser_headless_press_move_preview_cached_release_and_single_undo_redo() {
    for mode in [AppMode::Drawing, AppMode::Blackboard] {
        let (mut app, ctx) = eraser_app();
        app.mode = mode;
        let before = app.session.document.clone();
        let base = ink_mesh(&eraser_frame(&mut app, &ctx, vec![]));
        let start = Pos2::new(180.0, 150.0);
        let pressed = eraser_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(start), pointer(start, true)],
        );
        let press_mesh = ink_mesh(&pressed);
        let press_length = stroke_length(erased_document(&app));
        assert!(press_length < stroke_length(&before));
        assert_eq!(erased_document(&app).current_page().objects.len(), 2);
        assert_ne!(format!("{press_mesh:?}"), format!("{base:?}"));
        // 中间缺口来自真实分段网格，而不是透明画板上的白色遮盖。
        assert!(
            press_mesh
                .vertices
                .iter()
                .all(|v| (v.pos.x - start.x).abs() > 10.0)
        );
        assert_eq!(app.session.document, before);
        assert!(!app.session.history.can_undo());
        assert!(!app.session.history.is_dirty(&app.session.document));
        let end = Pos2::new(240.0, 150.0);
        let moved = eraser_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
        let preview = erased_document(&app).clone();
        assert!(stroke_length(&preview) < press_length);
        assert_eq!(app.session.document, before);
        let moved_mesh = ink_mesh(&moved);
        for events in [vec![], vec![egui::Event::PointerMoved(end)]] {
            let idle = eraser_frame(&mut app, &ctx, events);
            assert_eq!(erased_document(&app), &preview);
            assert!(std::sync::Arc::ptr_eq(&moved_mesh, &ink_mesh(&idle)));
        }
        let released = eraser_frame(&mut app, &ctx, vec![pointer(end, false)]);
        assert!(app.gesture.is_none());
        assert_eq!(app.session.document, preview);
        assert_eq!(app.session.document.revision, before.revision + 1);
        assert_eq!(
            format!("{:?}", ink_mesh(&released)),
            format!("{moved_mesh:?}")
        );
        let committed = eraser_frame(&mut app, &ctx, vec![]);
        assert_eq!(
            format!("{:?}", ink_mesh(&committed)),
            format!("{moved_mesh:?}")
        );
        app.undo(false);
        assert_eq!(app.session.document.pages, before.pages);
        assert!(!app.session.history.can_undo());
        assert!(!app.session.history.is_dirty(&app.session.document));
        assert_eq!(
            format!("{:?}", ink_mesh(&eraser_frame(&mut app, &ctx, vec![]))),
            format!("{base:?}")
        );
        app.undo(true);
        assert_eq!(app.session.document.pages, preview.pages);
        assert!(!app.session.history.can_redo());
    }
}

#[test]
fn eraser_headless_cancel_and_stale_context_restore_real_content() {
    for reason in [
        "escape",
        "tool",
        "page",
        "revision",
        "document",
        "collapse",
        "blocked_release",
    ] {
        let (mut app, ctx) = eraser_app();
        eraser_frame(&mut app, &ctx, vec![]);
        let start = Pos2::new(180.0, 150.0);
        eraser_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(start), pointer(start, true)],
        );
        assert!(stroke_length(erased_document(&app)) < 200.0);
        match reason {
            "escape" => {
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
            "tool" => app.tool = Tool::Pen,
            "page" => {
                app.session
                    .document
                    .pages
                    .push(board_core::Document::new().pages.remove(0));
                app.session.document.current_page = 1;
            }
            "revision" => {
                let page = app.session.document.current_page().id.clone();
                let revision = app.session.document.revision;
                let mut object = app.session.document.current_page().objects[0].clone();
                editing::translate(&mut object, Point { x: 0.0, y: 100.0 });
                app.session
                    .document
                    .apply(&page, revision, &[Operation::Update { object }])
                    .unwrap();
            }
            "document" => app.session.document = board_core::Document::new(),
            "collapse" => app.set_collapsed(&ctx, true),
            "blocked_release" => app.confirm_close = true,
            _ => unreachable!(),
        }
        let real = app.session.document.clone();
        eraser_frame(&mut app, &ctx, vec![pointer(start, false)]);
        assert!(app.gesture.is_none(), "{reason}");
        assert_eq!(app.session.document, real, "{reason}");
        assert!(!app.session.history.can_undo(), "{reason}");
        app.tool = Tool::Eraser;
        app.confirm_close = false;
        app.set_collapsed(&ctx, false);
        let output = eraser_frame(&mut app, &ctx, vec![egui::Event::PointerGone]);
        let actual = format!("{:?}", output.shapes);
        app.renderer = Default::default();
        let fresh = eraser_frame(&mut app, &ctx, vec![]);
        assert_eq!(actual, format!("{:?}", fresh.shapes), "{reason}");
    }
}

#[test]
fn eraser_headless_object_delete_cleans_connections_in_preview_and_history() {
    let (mut app, ctx) = eraser_app();
    app.session.document.pages[0].objects.extend([
        BoardObject {
            id: "target".into(),
            kind: ObjectKind::Shape {
                shape: ShapeKind::Rectangle,
                points: vec![
                    Point { x: 210.0, y: 310.0 },
                    Point { x: 290.0, y: 310.0 },
                    Point { x: 290.0, y: 380.0 },
                    Point { x: 210.0, y: 380.0 },
                ],
                style: Style::default(),
            },
        },
        BoardObject {
            id: "line".into(),
            kind: ObjectKind::Shape {
                shape: ShapeKind::Line,
                points: vec![Point { x: 290.0, y: 380.0 }, Point { x: 500.0, y: 450.0 }],
                style: Style::default(),
            },
        },
    ]);
    let page = app.session.document.current_page().id.clone();
    app.session
        .document
        .connect(
            &page,
            0,
            board_core::Connection {
                id: "connection".into(),
                page_id: page.clone(),
                line_id: "line".into(),
                line_endpoint: 0,
                target_id: "target".into(),
                target: board_core::Anchor::Vertex { index: 2 },
            },
        )
        .unwrap();
    app.session.history = board_core::History::new(&app.session.document);
    let before = app.session.document.clone();
    eraser_frame(&mut app, &ctx, vec![]);
    let start = Pos2::new(250.0, 150.0);
    eraser_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
    );
    let end = Pos2::new(250.0, 330.0);
    eraser_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
    let preview = erased_document(&app).clone();
    assert!(preview.connections.is_empty());
    assert!(
        !preview
            .current_page()
            .objects
            .iter()
            .any(|object| object.id == "target")
    );
    assert!(
        preview
            .current_page()
            .objects
            .iter()
            .any(|object| object.id == "line")
    );
    assert_eq!(app.session.document, before);
    eraser_frame(&mut app, &ctx, vec![pointer(end, false)]);
    assert_eq!(app.session.document, preview);
    app.undo(false);
    assert_eq!(app.session.document.pages, before.pages);
    assert_eq!(app.session.document.connections, before.connections);
    assert!(!app.session.history.can_undo());
    app.undo(true);
    assert_eq!(app.session.document.pages, preview.pages);
    assert!(app.session.document.connections.is_empty());
}

#[test]
fn eraser_headless_miss_does_not_create_history() {
    let (mut app, ctx) = eraser_app();
    let before = app.session.document.clone();
    eraser_frame(&mut app, &ctx, vec![]);
    let pos = Pos2::new(600.0, 500.0);
    eraser_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
    );
    assert_eq!(erased_document(&app), &before);
    eraser_frame(&mut app, &ctx, vec![pointer(pos, false)]);
    assert_eq!(app.session.document, before);
    assert!(!app.session.history.can_undo());
}

#[test]
fn preview_cancellation_and_same_page_deletion_do_not_pollute_revision_cache() {
    let mut app = app();
    let ctx = egui::Context::default();
    let render = |app: &mut BoardApp| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
                ..Default::default()
            },
            |ui| app.canvas(ui),
        );
        output.textures_delta.clear();
        format!("{:?}", output.shapes)
    };
    render(&mut app);
    let empty = render(&mut app);
    let context = ContextToken::capture(&app.session.document);
    app.gesture = Some(Gesture {
        document: context.document_id,
        page: context.page_id,
        revision: context.revision,
        points: vec![StrokePoint {
            x: 40.0,
            y: 40.0,
            time: 0.0,
            pressure: 1.0,
        }],
        original: None,
        vertex: None,
        resize: None,
        erasing: None,
    });
    assert_ne!(render(&mut app), empty);
    app.gesture = None;
    assert_eq!(render(&mut app), empty);
    app.add(ObjectKind::Stroke {
        points: vec![StrokePoint {
            x: 40.0,
            y: 40.0,
            time: 0.0,
            pressure: 1.0,
        }],
        style: Default::default(),
    });
    assert_ne!(render(&mut app), empty);
    let id = app.session.document.current_page().objects[0].id.clone();
    app.apply(vec![Operation::Delete { id }]);
    assert_eq!(render(&mut app), empty);
}

#[test]
fn pen_overlay_reuses_base_mesh_and_erase_undo_restore_it() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.add(ObjectKind::Stroke {
        points: vec![
            StrokePoint {
                x: 40.0,
                y: 40.0,
                time: 0.0,
                pressure: 1.0,
            },
            StrokePoint {
                x: 140.0,
                y: 80.0,
                time: 1.0,
                pressure: 0.5,
            },
        ],
        style: app.style,
    });
    app.selected = None;
    let render = |app: &mut BoardApp| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
                ..Default::default()
            },
            |ui| app.canvas(ui),
        );
        output.textures_delta.clear();
        output
            .shapes
            .into_iter()
            .find_map(|shape| match shape.shape {
                egui::Shape::Mesh(mesh) => Some(mesh),
                _ => None,
            })
    };
    let base = render(&mut app).unwrap();
    let document = app.session.document.clone();
    app.gesture = Some(Gesture {
        document: document.id.clone(),
        page: document.current_page().id.clone(),
        revision: document.revision,
        points: vec![StrokePoint {
            x: 90.0,
            y: 60.0,
            time: 0.0,
            pressure: 1.0,
        }],
        original: None,
        vertex: None,
        resize: None,
        erasing: None,
    });
    let preview_base = render(&mut app).unwrap();
    assert!(std::sync::Arc::ptr_eq(&base, &preview_base));
    assert_eq!(app.session.document, document);
    app.gesture = None;
    assert!(std::sync::Arc::ptr_eq(&base, &render(&mut app).unwrap()));
    let original = document.current_page().objects[0].clone();
    app.tool = Tool::Select;
    app.gesture = Some(Gesture {
        document: document.id.clone(),
        page: document.current_page().id.clone(),
        revision: document.revision,
        points: vec![
            StrokePoint {
                x: 40.0,
                y: 40.0,
                time: 0.0,
                pressure: 1.0,
            },
            StrokePoint {
                x: 70.0,
                y: 40.0,
                time: 1.0,
                pressure: 1.0,
            },
        ],
        original: Some(original),
        vertex: None,
        resize: None,
        erasing: None,
    });
    assert_ne!(
        format!("{:?}", render(&mut app).unwrap()),
        format!("{base:?}")
    );
    app.gesture = None;
    assert_eq!(
        format!("{:?}", render(&mut app).unwrap()),
        format!("{base:?}")
    );
    app.tool = Tool::Eraser;
    let ops = editing::erase_operations(
        app.session.document.current_page(),
        &[Point { x: 90.0, y: 60.0 }],
        200.0,
    )
    .unwrap();
    assert!(!ops.is_empty());
    app.apply(ops);
    assert!(render(&mut app).is_none());
    app.undo(false);
    assert_eq!(
        format!("{:?}", render(&mut app).unwrap()),
        format!("{base:?}")
    );
    app.undo(true);
    assert!(render(&mut app).is_none());
}
