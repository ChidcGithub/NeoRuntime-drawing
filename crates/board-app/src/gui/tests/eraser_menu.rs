fn eraser_page_fixture() -> BoardApp {
    let mut app = app();
    let asset = app.session.resources.import_png(capture_png()).unwrap();
    write_sample(&mut app, Instant::now());
    app.add(ObjectKind::Text {
        position: Point { x: 30.0, y: 180.0 },
        text: "text".into(),
        size: 24.0,
        color: Color::default(),
    });
    app.add(ObjectKind::Image {
        position: Point { x: 300.0, y: 180.0 },
        width: 40.0,
        height: 40.0,
        asset_ref: asset,
    });
    app.add(ObjectKind::Math {
        position: Point { x: 400.0, y: 180.0 },
        layout: board_core::MathLayout::Text("5/6".into()),
        size: 24.0,
        color: Color::default(),
    });
    app.add(ObjectKind::CoordinateSystem {
        origin: Point { x: 500.0, y: 180.0 },
        scale: 20.0,
    });
    app.add(drag_plot("plot", 600.0, &["x"]).kind);
    let mut shapes = reverse_drag_objects();
    let vertices = editing::vertices(&shapes[0]);
    if let ObjectKind::Shape { points, .. } = &mut shapes[0].kind {
        *points = vertices;
    }
    app.session.document.pages[0].objects.extend(shapes.clone());
    bind(&mut app, "line", 0, "shape", 0);
    app.session.document.add_page().unwrap();
    let other_shapes: Vec<_> = shapes
        .into_iter()
        .map(|mut object| {
            object.id = format!("other-{}", object.id);
            object
        })
        .collect();
    app.session.document.pages[1].objects = other_shapes;
    bind(&mut app, "other-line", 0, "other-shape", 0);
    app.session.document.set_current_page(0).unwrap();
    app.session.history = board_core::History::new(&app.session.document);
    app.selected = Some("shape".into());
    app
}

#[test]
fn eraser_all_and_page_clear_remove_only_current_page_with_one_undo_redo() {
    for mode in [AppMode::Drawing, AppMode::Blackboard] {
        for page_menu in [false, true] {
            let mut app = eraser_page_fixture();
            app.mode = mode;
            let ctx = egui::Context::default();
            let before = app.session.document.clone();
            if page_menu {
                click_text(&mut app, &ctx, "1/2");
                assert!(egui::Popup::is_any_open(&ctx));
                click_text(&mut app, &ctx, "清屏（可撤销）");
            } else {
                open_toolbar_menu(&mut app, &ctx, "橡皮");
                click_text(&mut app, &ctx, "全部擦除");
                assert!(!egui::Popup::is_any_open(&ctx));
                assert!(app.tool == Tool::Eraser);
            }
            let cleared = app.session.document.clone();
            assert!(cleared.current_page().objects.is_empty());
            assert_eq!(cleared.id, before.id);
            assert_eq!(cleared.current_page, before.current_page);
            assert_eq!(cleared.pages.len(), before.pages.len());
            assert_eq!(cleared.pages[0].id, before.pages[0].id);
            assert_eq!(cleared.pages[1], before.pages[1]);
            assert_eq!(cleared.connections, vec![before.connections[1].clone()]);
            assert_eq!(cleared.revision, before.revision + 1);
            assert!(app.selected.is_none() && app.gesture.is_none());
            cleared.validate().unwrap();
            assert!(app.session.history.is_dirty(&cleared));
            app.undo(false);
            assert_eq!(app.session.document.pages, before.pages);
            assert_eq!(app.session.document.connections, before.connections);
            assert!(!app.session.history.can_undo());
            assert!(!app.session.history.is_dirty(&app.session.document));
            app.undo(true);
            assert_eq!(app.session.document.pages, cleared.pages);
            assert_eq!(app.session.document.connections, cleared.connections);
            assert!(!app.session.history.can_redo());
            assert_eq!(app.session.document.revision, before.revision + 3);
        }
    }
}

#[test]
fn eraser_more_submenu_is_single_click_and_shares_clear_action() {
    let mut app = eraser_page_fixture();
    let ctx = egui::Context::default();
    let before = app.session.document.clone();
    open_toolbar_menu(&mut app, &ctx, "更多 / 状态");
    click_text(&mut app, &ctx, "橡皮");
    click_text(&mut app, &ctx, "全部擦除");
    assert!(app.session.document.current_page().objects.is_empty());
    assert_eq!(app.session.document.pages[1], before.pages[1]);
    assert_eq!(app.session.document.revision, before.revision + 1);
    app.undo(false);
    assert_eq!(app.session.document.pages, before.pages);
    assert_eq!(app.session.document.connections, before.connections);
    assert!(!app.session.history.can_undo());
}

#[test]
fn eraser_empty_page_does_not_add_history_or_discard_redo() {
    for with_redo in [false, true] {
        let mut app = app();
        let ctx = egui::Context::default();
        if with_redo {
            app.add(drag_plot("plot", 200.0, &["x"]).kind);
            app.undo(false);
        }
        let before = app.session.document.clone();
        open_toolbar_menu(&mut app, &ctx, "橡皮");
        click_text(&mut app, &ctx, "全部擦除");
        click_text(&mut app, &ctx, "1/1");
        click_text(&mut app, &ctx, "清屏（可撤销）");
        assert_eq!(app.session.document, before);
        assert!(!app.session.history.can_undo());
        assert_eq!(app.session.history.can_redo(), with_redo);
        assert!(!app.session.history.is_dirty(&app.session.document));
        if with_redo {
            app.undo(true);
            assert_eq!(app.session.document.current_page().objects.len(), 1);
        }
    }
}

#[test]
fn eraser_clear_supports_pages_above_operation_limit_up_to_document_limit() {
    for count in [
        board_core::MAX_OPERATIONS + 1,
        board_core::MAX_DOCUMENT_OBJECTS,
    ] {
        let mut app = app();
        app.session.document.pages[0].objects = (0..count)
            .map(|index| BoardObject {
                id: format!("text-{index}"),
                kind: ObjectKind::Text {
                    position: Point::default(),
                    text: "x".into(),
                    size: 12.0,
                    color: Color::default(),
                },
            })
            .collect();
        app.session.document.validate().unwrap();
        app.session.history = board_core::History::new(&app.session.document);
        let before = app.session.document.clone();
        app.clear_current_page();
        assert!(app.session.document.current_page().objects.is_empty());
        assert_eq!(app.session.document.revision, before.revision + 1);
        app.undo(false);
        assert_eq!(app.session.document.pages, before.pages);
        assert!(!app.session.history.can_undo());
        app.undo(true);
        assert!(app.session.document.current_page().objects.is_empty());
        assert!(!app.session.history.can_redo());
    }
}

#[test]
fn eraser_clear_failure_is_atomic() {
    let mut app = eraser_page_fixture();
    app.session.document.revision = u64::MAX;
    app.session.history = board_core::History::new(&app.session.document);
    let before = app.session.document.clone();
    let selected = app.selected.clone();
    app.clear_current_page();
    assert_eq!(app.session.document, before);
    assert_eq!(app.selected, selected);
    assert!(!app.session.history.can_undo());
    assert!(!app.session.history.can_redo());
    assert_eq!(app.status, board_core::Error::RevisionOverflow.to_string());
}
