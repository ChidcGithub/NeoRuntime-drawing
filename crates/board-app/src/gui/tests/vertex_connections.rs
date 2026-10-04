#[test]
fn reverse_vertex_headless_drag_connects_moves_and_undoes_once() {
    let (mut app, ctx) = plot_drag_app(reverse_drag_objects());
    app.selected = Some("shape".into());
    let before = app.session.document.clone();
    let end = Pos2::new(300.0, 100.0);
    let output = drag_to(&mut app, &ctx, Pos2::new(200.0, 100.0), end);
    assert!(format!("{:?}", output.shapes).contains("持久端点连接"));
    assert_eq!(app.session.document, before);
    let mut expected = before.clone();
    app.polygon_vertex_edit()
        .unwrap()
        .unwrap()
        .apply(&mut expected)
        .unwrap();
    release(&mut app, &ctx, end);
    let connected = app.session.document.clone();
    assert_eq!(connected.revision, 1);
    assert_eq!(
        connected.current_page().objects,
        expected.current_page().objects
    );
    assert_eq!(connected.connections.len(), 1);
    let c = &connected.connections[0];
    assert_eq!(
        (&*c.line_id, c.line_endpoint, &*c.target_id),
        ("line", 0, "shape")
    );
    assert_eq!(c.target, board_core::Anchor::Vertex { index: 1 });
    app.undo(false);
    assert_eq!(app.session.document.pages, before.pages);
    assert!(app.session.document.connections.is_empty());
    assert!(!app.session.history.can_undo());
    assert!(!app.session.history.is_dirty(&app.session.document));
    app.undo(true);
    assert_eq!(app.session.document.pages, connected.pages);
    assert_eq!(app.session.document.connections, connected.connections);
    // 第二次真实顶点拖动：已有入边在预览和提交中均由 core 传播。
    let next = Pos2::new(330.0, 130.0);
    drag_to(&mut app, &ctx, end, next);
    let mut expected = app.session.document.clone();
    app.polygon_vertex_edit()
        .unwrap()
        .unwrap()
        .apply(&mut expected)
        .unwrap();
    assert_ne!(
        shape_points(&expected.current_page().objects[1])[0],
        Point { x: end.x, y: end.y }
    );
    release(&mut app, &ctx, next);
    assert_eq!(app.session.document.pages, expected.pages);
    assert_eq!(app.session.document.connections, connected.connections);
    let mut moved = app.session.document.current_page().objects[0].clone();
    editing::translate(&mut moved, Point { x: 20.0, y: 30.0 });
    app.apply(vec![Operation::Update { object: moved }]);
    assert_eq!(
        shape_points(&app.session.document.current_page().objects[1])[0],
        shape_points(&app.session.document.current_page().objects[0])[1]
    );
}

#[test]
fn reverse_vertex_rejects_dimension_conflict_through_line_chain() {
    for dragged in [ShapeKind::Rectangle, ShapeKind::Cube] {
        let mut objects = reverse_drag_objects();
        if let ObjectKind::Shape { shape, .. } = &mut objects[0].kind {
            *shape = dragged;
        }
        let opposite = if dragged == ShapeKind::Cube {
            ShapeKind::Rectangle
        } else {
            ShapeKind::Cube
        };
        let mut target = edit_shape(
            opposite,
            Point { x: 500.0, y: 300.0 },
            Point { x: 600.0, y: 400.0 },
        );
        target.id = "other".into();
        let vertices = editing::vertices(&target);
        if let ObjectKind::Shape { points, .. } = &mut target.kind {
            *points = vertices;
        }
        let mut bridge = objects[1].clone();
        bridge.id = "bridge".into();
        objects.extend([target, bridge]);
        let start = editing::vertices(&objects[0])[1];
        let (mut app, ctx) = plot_drag_app(objects);
        bind(&mut app, "bridge", 1, "other", 0);
        bind(&mut app, "line", 1, "bridge", 0);
        let before = app.session.document.clone();
        app.selected = Some("shape".into());
        let end = Pos2::new(300.0, 100.0);
        drag_to(&mut app, &ctx, Pos2::new(start.x, start.y), end);
        release(&mut app, &ctx, end);
        assert_eq!(app.session.document, before);
        assert!(app.status.contains("2D 与 3D"));
        assert!(!app.session.history.can_undo());
    }
}

#[test]
fn reverse_curve_resize_handle_never_creates_connection() {
    let mut objects = reverse_drag_objects();
    if let ObjectKind::Shape { shape, .. } = &mut objects[0].kind {
        *shape = ShapeKind::Circle;
    }
    let (mut app, ctx) = plot_drag_app(objects);
    app.selected = Some("shape".into());
    let end = Pos2::new(300.0, 100.0);
    drag_to(&mut app, &ctx, Pos2::new(200.0, 100.0), end);
    assert!(app.gesture.as_ref().unwrap().vertex.is_none());
    assert!(app.polygon_vertex_edit().is_none());
    release(&mut app, &ctx, end);
    assert!(app.session.document.connections.is_empty());
}

#[test]
fn reverse_vertex_line_interior_is_explicitly_geometry_only() {
    let mut objects = reverse_drag_objects();
    if let ObjectKind::Shape { points, .. } = &mut objects[1].kind {
        *points = vec![Point { x: 250.0, y: 100.0 }, Point { x: 450.0, y: 100.0 }];
    }
    let (mut app, ctx) = plot_drag_app(objects);
    app.selected = Some("shape".into());
    let end = Pos2::new(350.0, 100.0);
    let output = drag_to(&mut app, &ctx, Pos2::new(200.0, 100.0), end);
    assert!(format!("{:?}", output.shapes).contains("仅几何吸附"));
    release(&mut app, &ctx, end);
    assert!(app.status.contains("线段内部"));
    assert!(app.session.document.connections.is_empty());
    assert_eq!(app.session.document.revision, 1);
}

#[test]
fn reverse_vertex_rejects_mixed_components_and_bound_endpoints_atomically() {
    for occupied in [false, true] {
        for kind in [ShapeKind::Cube, ShapeKind::Rectangle] {
            if !occupied && kind == ShapeKind::Rectangle {
                continue;
            }
            let mut objects = reverse_drag_objects();
            let mut target = edit_shape(
                kind,
                Point { x: 500.0, y: 300.0 },
                Point { x: 600.0, y: 400.0 },
            );
            target.id = "other".into();
            let vertex = editing::vertices(&target)[0];
            editing::move_vertex(
                &mut target,
                0,
                Point {
                    x: vertex.x + 1.0,
                    y: vertex.y,
                },
            );
            objects.push(target);
            let (mut app, ctx) = plot_drag_app(objects);
            bind(&mut app, "line", usize::from(!occupied), "other", 0);
            let before = app.session.document.clone();
            let p = shape_points(&before.current_page().objects[1])[0];
            let end = Pos2::new(p.x, p.y);
            app.selected = Some("shape".into());
            let output = drag_to(&mut app, &ctx, Pos2::new(200.0, 100.0), end);
            // 已占用端点与其它图形顶点重合，剔除后不允许伪装为几何吸附。
            if occupied {
                assert!(app.polygon_vertex_edit().unwrap().is_err());
            }
            assert!(format!("{:?}", output.shapes).contains("取消"));
            release(&mut app, &ctx, end);
            assert_eq!(app.session.document, before);
            assert!(!app.session.history.can_undo());
            assert!(!app.session.history.is_dirty(&app.session.document));
        }
    }
}

#[test]
fn reverse_vertex_candidates_choose_nearest_free_endpoint_deterministically() {
    for reorder in [false, true] {
        let mut objects = reverse_drag_objects();
        if let ObjectKind::Shape { points, .. } = &mut objects[1].kind {
            *points = vec![Point { x: 300.0, y: 100.0 }, Point { x: 305.0, y: 100.0 }];
        }
        let mut target = edit_shape(
            ShapeKind::Rectangle,
            Point { x: 300.0, y: 100.0 },
            Point { x: 400.0, y: 200.0 },
        );
        target.id = "other".into();
        if let ObjectKind::Shape { points, .. } = &mut target.kind {
            *points = editing::vertices(&objects[0]);
            points[0] = Point { x: 300.0, y: 100.0 };
        }
        objects.push(target);
        if reorder {
            objects.swap(1, 2);
        }
        let (mut app, ctx) = plot_drag_app(objects);
        bind(&mut app, "line", 0, "other", 0);
        let old = app.session.document.connections.clone();
        app.selected = Some("shape".into());
        let end = Pos2::new(300.0, 100.0);
        drag_to(&mut app, &ctx, Pos2::new(200.0, 100.0), end);
        let edit = app.polygon_vertex_edit().unwrap().unwrap();
        assert_eq!(edit.connection.unwrap().line_endpoint, 1);
        release(&mut app, &ctx, end);
        assert_eq!(app.session.document.connections.len(), 2);
        assert_eq!(app.session.document.connections[0], old[0]);
        assert_eq!(
            shape_points(&app.session.document.current_page().objects[0])[1],
            Point { x: 305.0, y: 100.0 }
        );
    }
    let mut objects = reverse_drag_objects();
    let mut other = objects[1].clone();
    other.id = "second".into();
    objects.push(other);
    let (mut app, ctx) = plot_drag_app(objects);
    app.selected = Some("shape".into());
    let end = Pos2::new(300.0, 100.0);
    drag_to(&mut app, &ctx, Pos2::new(200.0, 100.0), end);
    assert_eq!(
        app.polygon_vertex_edit()
            .unwrap()
            .unwrap()
            .connection
            .unwrap()
            .line_id,
        "line"
    );
    release(&mut app, &ctx, end);
    assert_eq!(app.session.document.connections[0].line_id, "line");
}

#[test]
fn reverse_vertex_preserves_dependency_chain_without_cycles_or_stolen_bindings() {
    let mut objects = reverse_drag_objects();
    let p = editing::vertices(&objects[0])[0];
    editing::move_vertex(
        &mut objects[0],
        0,
        Point {
            x: p.x + 1.0,
            y: p.y,
        },
    );
    let mut other = edit_shape(
        ShapeKind::Line,
        Point { x: 500.0, y: 100.0 },
        Point { x: 600.0, y: 200.0 },
    );
    other.id = "second".into();
    objects.push(other);
    let (mut app, ctx) = plot_drag_app(objects);
    bind(&mut app, "line", 1, "shape", 0);
    bind(&mut app, "second", 1, "line", 0);
    let before = app.session.document.clone();
    app.selected = Some("shape".into());
    let end = Pos2::new(500.0, 100.0);
    drag_to(&mut app, &ctx, Pos2::new(200.0, 100.0), end);
    release(&mut app, &ctx, end);
    assert_eq!(app.session.document.revision, before.revision + 1);
    assert_eq!(&app.session.document.connections[..2], before.connections);
    assert_eq!(app.session.document.connections.len(), 3);
    app.session.document.validate().unwrap();
    app.undo(false);
    assert_eq!(app.session.document.pages, before.pages);
    assert_eq!(app.session.document.connections, before.connections);
    // 两端都占用；拖向依赖链上的已占用端点也不能替换绑定。
    bind(&mut app, "second", 0, "line", 0);
    bind(&mut app, "line", 0, "shape", 0);
    let before = app.session.document.clone();
    let end = Pos2::new(101.0, 100.0);
    drag_to(&mut app, &ctx, Pos2::new(200.0, 100.0), end);
    release(&mut app, &ctx, end);
    assert_eq!(app.session.document, before);
    assert!(app.status.contains("不会替换"));
    assert!(!app.session.history.can_undo());
    app.session.document.validate().unwrap();
}

#[test]
fn reverse_vertex_stale_and_stationary_never_write() {
    for stale in 0..4 {
        let (mut app, ctx) = plot_drag_app(reverse_drag_objects());
        app.selected = Some("shape".into());
        let end = if stale == 3 {
            Pos2::new(200.0, 100.0)
        } else {
            Pos2::new(300.0, 100.0)
        };
        drag_to(&mut app, &ctx, Pos2::new(200.0, 100.0), end);
        match stale {
            0 => app.session.document.id = new_id(),
            1 => {
                app.session.document.add_page().unwrap();
            }
            2 => app.session.document.revision += 1,
            _ => {}
        }
        let before = app.session.document.clone();
        if stale != 3 {
            assert!(app.preview().is_none());
        }
        release(&mut app, &ctx, end);
        assert_eq!(app.session.document, before);
        assert!(!app.session.history.can_undo());
    }
    let mut objects = reverse_drag_objects();
    if let ObjectKind::Shape { points, .. } = &mut objects[1].kind {
        points[0] = Point { x: 200.0, y: 100.0 };
    }
    let (mut app, ctx) = plot_drag_app(objects);
    app.selected = Some("shape".into());
    let before = app.session.document.clone();
    let end = Pos2::new(200.0, 100.0);
    drag_to(&mut app, &ctx, end, end);
    release(&mut app, &ctx, end);
    assert_eq!(app.session.document, before);
    assert!(!app.session.history.is_dirty(&app.session.document));
}
