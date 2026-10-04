#[test]
fn bbox_curve_resize_keeps_circle_round_and_ellipse_axes_independent() {
    for (shape, expected) in [
        (ShapeKind::Circle, (130.0, 130.0)),
        (ShapeKind::Ellipse, (150.0, 170.0)),
    ] {
        let object = edit_shape(
            shape,
            Point { x: 100.0, y: 100.0 },
            Point { x: 200.0, y: 180.0 },
        );
        let (mut app, ctx) = plot_drag_app(vec![object.clone()]);
        app.selected = Some(object.id.clone());
        let handle = editing::resize_handles(&object)[2];
        // Circle uses its rendered 80x80 bounds, not the raw 100x80 drag box.
        assert_eq!(
            handle.x,
            if shape == ShapeKind::Circle {
                180.0
            } else {
                200.0
            }
        );
        let start = Pos2::new(handle.x, handle.y);
        let end = start + Vec2::new(50.0, 90.0);
        let output = drag_to(&mut app, &ctx, start, end);
        assert_eq!(app.gesture.as_ref().unwrap().resize, Some(2));
        assert_eq!(app.gesture.as_ref().unwrap().vertex, None);
        let preview = app.preview().unwrap();
        let handles = editing::resize_handles(&preview);
        let size = (handles[2].x - handles[0].x, handles[2].y - handles[0].y);
        assert_eq!(size, expected);
        assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == Color32::LIGHT_BLUE && r.rect.center() == Pos2::new(handles[2].x, handles[2].y))));
        assert_eq!(
            app.session.document.current_page().objects,
            vec![object.clone()]
        );
        release(&mut app, &ctx, end);
        assert_eq!(app.session.document.revision, 1);
        assert_eq!(
            app.session.document.current_page().objects,
            vec![preview.clone()]
        );
        app.undo(false);
        assert_eq!(app.session.document.current_page().objects, vec![object]);
        assert!(!app.session.history.can_undo());
        app.undo(true);
        assert_eq!(app.session.document.current_page().objects, vec![preview]);
        assert!(!app.session.history.can_redo());
    }
}

#[test]
fn materialized_curve_resize_preserves_indices_connections_and_one_history_step() {
    for shape in [ShapeKind::Circle, ShapeKind::Ellipse] {
        for edge in [false, true] {
            let mut object = edit_shape(
                shape,
                Point { x: 100.0, y: 100.0 },
                Point { x: 200.0, y: 200.0 },
            );
            let mut points = board_ink::shape_geometry(
                shape,
                shape_points(&object)[0],
                shape_points(&object)[1],
            )
            .unwrap()
            .vertices;
            // Rotated index order proves resizing does not regenerate canonical samples.
            points.rotate_left(7);
            if let ObjectKind::Shape { points: stored, .. } = &mut object.kind {
                *stored = points.clone();
            }
            let mut line = edit_shape(ShapeKind::Line, points[9], Point { x: 500.0, y: 400.0 });
            line.id = "line".into();
            let (mut app, ctx) = plot_drag_app(vec![object.clone(), line]);
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
                        target_id: object.id.clone(),
                        target: if edge {
                            board_core::Anchor::Edge {
                                start: 9,
                                end: 10,
                                t: 0.3,
                            }
                        } else {
                            board_core::Anchor::Vertex { index: 9 }
                        },
                    },
                )
                .unwrap();
            app.session.history = board_core::History::new(&app.session.document);
            let before = app.session.document.clone();
            app.selected = Some(object.id.clone());
            // Cross the fixed corner on both axes, preserving reflected sample indices.
            let end = Pos2::new(60.0, 40.0);
            drag_to(&mut app, &ctx, Pos2::new(200.0, 200.0), end);
            release(&mut app, &ctx, end);
            let after = app.session.document.clone();
            assert_eq!(after.revision, before.revision + 1);
            assert_eq!(after.connections, before.connections);
            let resized = shape_points(&after.current_page().objects[0]);
            assert_eq!(resized.len(), points.len());
            for (old, new) in points.iter().zip(resized) {
                assert!((new.x - (100.0 - (old.x - 100.0) * 0.4)).abs() < 0.001);
                let sy = if shape == ShapeKind::Circle { 0.4 } else { 0.6 };
                assert!((new.y - (100.0 - (old.y - 100.0) * sy)).abs() < 0.001);
            }
            let anchor = if edge {
                Point {
                    x: resized[9].x + (resized[10].x - resized[9].x) * 0.3,
                    y: resized[9].y + (resized[10].y - resized[9].y) * 0.3,
                }
            } else {
                resized[9]
            };
            let attached = shape_points(&after.current_page().objects[1])[0];
            assert!(
                (attached.x - anchor.x).abs() < 0.001 && (attached.y - anchor.y).abs() < 0.001
            );
            app.undo(false);
            assert_eq!(
                app.session.document.current_page().objects,
                before.current_page().objects
            );
            assert_eq!(app.session.document.connections, before.connections);
            assert!(!app.session.history.can_undo());
            app.undo(true);
            assert_eq!(
                app.session.document.current_page().objects,
                after.current_page().objects
            );
        }
    }
}

#[test]
fn curve_resize_stale_context_never_previews_or_writes() {
    for stale in 0..3 {
        let object = edit_shape(
            ShapeKind::Ellipse,
            Point { x: 100.0, y: 100.0 },
            Point { x: 200.0, y: 200.0 },
        );
        let (mut app, ctx) = plot_drag_app(vec![object]);
        app.selected = Some("shape".into());
        let end = Pos2::new(250.0, 260.0);
        drag_to(&mut app, &ctx, Pos2::new(200.0, 200.0), end);
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
    }
}

#[test]
fn cube_vertex_three_does_not_snap_to_non_neighbor_four() {
    let mut object = edit_shape(
        ShapeKind::Cube,
        Point::default(),
        Point { x: 100.0, y: 100.0 },
    );
    if let ObjectKind::Shape { points, .. } = &mut object.kind {
        *points = vec![
            Point { x: 100.0, y: 260.0 },
            Point { x: 400.0, y: 120.0 },
            Point { x: 310.0, y: 260.0 },
            Point { x: 200.0, y: 320.0 },
            Point { x: 200.0, y: 200.0 },
            Point { x: 500.0, y: 200.0 },
            Point { x: 500.0, y: 400.0 },
            Point { x: 300.0, y: 340.0 },
        ];
    }
    let target = Point { x: 201.0, y: 300.0 };
    assert_ne!(
        board_ink::snap_angle(shape_points(&object)[4], target, 6.0).unwrap(),
        target
    );
    let (mut app, ctx) = plot_drag_app(vec![object.clone()]);
    app.selected = Some("shape".into());
    let end = Pos2::new(target.x, target.y);
    drag_to(&mut app, &ctx, Pos2::new(200.0, 320.0), end);
    assert_eq!(app.gesture.as_ref().unwrap().vertex, Some(3));
    assert_eq!(shape_points(&app.preview().unwrap())[3], target);
    release(&mut app, &ctx, end);
    let mut expected = shape_points(&object).to_vec();
    expected[3] = target;
    assert_eq!(
        shape_points(&app.session.document.current_page().objects[0]),
        expected
    );
}

#[test]
fn polygon_vertex_edit_still_materializes_and_snaps_to_real_neighbor() {
    let object = edit_shape(
        ShapeKind::Rectangle,
        Point { x: 100.0, y: 100.0 },
        Point { x: 200.0, y: 200.0 },
    );
    let (mut app, ctx) = plot_drag_app(vec![object.clone()]);
    app.selected = Some("shape".into());
    let end = Pos2::new(230.0, 102.0);
    drag_to(&mut app, &ctx, Pos2::new(200.0, 100.0), end);
    assert_eq!(app.gesture.as_ref().unwrap().vertex, Some(1));
    assert_eq!(app.gesture.as_ref().unwrap().resize, None);
    let expected = board_ink::snap_angle(
        Point { x: 100.0, y: 100.0 },
        Point { x: end.x, y: end.y },
        6.0,
    )
    .unwrap();
    release(&mut app, &ctx, end);
    let points = shape_points(&app.session.document.current_page().objects[0]);
    assert_eq!(
        points,
        &[
            Point { x: 100.0, y: 100.0 },
            expected,
            Point { x: 200.0, y: 200.0 },
            Point { x: 100.0, y: 200.0 }
        ]
    );
}

#[test]
fn tiny_curves_have_separate_resize_and_selection_move_targets() {
    for shape in [ShapeKind::Circle, ShapeKind::Ellipse] {
        let object = edit_shape(
            shape,
            Point { x: 100.0, y: 100.0 },
            Point { x: 104.0, y: 104.0 },
        );
        let (mut app, ctx) = plot_drag_app(vec![object.clone()]);
        // First select by clicking the curve, then move while selected.
        let center = Pos2::new(102.0, 102.0);
        drag_to(&mut app, &ctx, center, center);
        release(&mut app, &ctx, center);
        assert_eq!(app.selected.as_deref(), Some("shape"));
        assert_eq!(app.session.document.revision, 0);
        let end = center + Vec2::new(30.0, 20.0);
        drag_to(&mut app, &ctx, center, end);
        assert_eq!(app.gesture.as_ref().unwrap().resize, None);
        release(&mut app, &ctx, end);
        let mut moved = object;
        editing::translate(&mut moved, Point { x: 30.0, y: 20.0 });
        assert_eq!(
            app.session.document.current_page().objects,
            vec![moved.clone()]
        );
        let handle = editing::resize_handles(&moved)[2];
        let start = Pos2::new(handle.x, handle.y);
        let end = start + Vec2::new(10.0, 20.0);
        drag_to(&mut app, &ctx, start, end);
        assert_eq!(app.gesture.as_ref().unwrap().resize, Some(2));
        release(&mut app, &ctx, end);
        assert_eq!(
            shape_points(&app.session.document.current_page().objects[0]).len(),
            64
        );
        assert_eq!(app.session.document.revision, 2);
    }
}

#[test]
fn reversed_bbox_curves_resize_from_each_corner() {
    for shape in [ShapeKind::Circle, ShapeKind::Ellipse] {
        for index in 0..4 {
            let object = edit_shape(
                shape,
                Point { x: 240.0, y: 220.0 },
                Point { x: 100.0, y: 100.0 },
            );
            let handles = editing::resize_handles(&object);
            let anchor = handles[(index + 2) % 4];
            let (mut app, ctx) = plot_drag_app(vec![object]);
            app.selected = Some("shape".into());
            let start = Pos2::new(handles[index].x, handles[index].y);
            let end = start
                + Vec2::new(
                    if index == 0 || index == 3 {
                        -20.0
                    } else {
                        20.0
                    },
                    if index < 2 { -30.0 } else { 30.0 },
                );
            drag_to(&mut app, &ctx, start, end);
            assert_eq!(app.gesture.as_ref().unwrap().resize, Some(index));
            release(&mut app, &ctx, end);
            let resized =
                editing::resize_handles(&app.session.document.current_page().objects[0]);
            assert_eq!(resized[(index + 2) % 4], anchor);
            assert_eq!(
                resized[2].x - resized[0].x,
                if shape == ShapeKind::Circle {
                    140.0
                } else {
                    160.0
                }
            );
            assert_eq!(
                resized[2].y - resized[0].y,
                if shape == ShapeKind::Circle {
                    140.0
                } else {
                    150.0
                }
            );
        }
    }
}

#[test]
fn zero_size_curve_gestures_are_safe_and_collapsed_axes_stay_collapsed() {
    for shape in [ShapeKind::Circle, ShapeKind::Ellipse] {
        for materialized in [false, true] {
            let mut object = edit_shape(
                shape,
                Point { x: 100.0, y: 100.0 },
                Point { x: 100.0, y: 100.0 },
            );
            if materialized && let ObjectKind::Shape { points, .. } = &mut object.kind {
                *points = board_ink::shape_geometry(shape, points[0], points[1])
                    .unwrap()
                    .vertices;
            }
            let handle = editing::resize_handles(&object)[2];
            let (mut app, ctx) = plot_drag_app(vec![object.clone()]);
            app.selected = Some("shape".into());
            let end = Pos2::new(handle.x + 50.0, handle.y + 40.0);
            drag_to(&mut app, &ctx, Pos2::new(handle.x, handle.y), end);
            assert_eq!(app.gesture.as_ref().unwrap().resize, Some(2));
            release(&mut app, &ctx, end);
            assert_eq!(app.session.document.current_page().objects, vec![object]);
            assert_eq!(app.session.document.revision, 0);
            assert!(!app.session.history.can_undo());
        }
    }
}

#[test]
fn curve_bbox_corners_are_not_connection_anchors() {
    let object = edit_shape(
        ShapeKind::Circle,
        Point { x: 100.0, y: 100.0 },
        Point { x: 300.0, y: 300.0 },
    );
    let (mut app, ctx) = plot_drag_app(vec![object]);
    app.selected = Some("shape".into());
    app.tool = Tool::Shape;
    app.shape = ShapeKind::Line;
    let end = Pos2::new(40.0, 40.0);
    drag_to(&mut app, &ctx, Pos2::new(100.0, 100.0), end);
    release(&mut app, &ctx, end);
    assert_eq!(app.session.document.current_page().objects.len(), 2);
    assert!(app.session.document.connections.is_empty());
    assert_eq!(
        shape_points(&app.session.document.current_page().objects[0]).len(),
        2
    );
}
