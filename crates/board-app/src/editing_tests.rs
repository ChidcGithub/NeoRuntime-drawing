use super::*;
use board_core::{Document, History, StrokePoint, Style};
#[test]
fn curve_resize_rejects_nonfinite_overflow_and_preserves_collapsed_axes() {
    for kind in [ShapeKind::Circle, ShapeKind::Ellipse] {
        let original = shape(
            "curve",
            kind,
            Point { x: 100.0, y: 100.0 },
            Point { x: 200.0, y: 200.0 },
        );
        for delta in [
            Point {
                x: f32::NAN,
                y: 1.0,
            },
            Point {
                x: f32::INFINITY,
                y: 1.0,
            },
            Point {
                x: f32::MAX,
                y: f32::MAX,
            },
        ] {
            let mut object = original.clone();
            resize_curve(&mut object, 2, delta);
            assert_eq!(object, original);
        }
        let mut collapsed = original;
        resize_curve(
            &mut collapsed,
            2,
            Point {
                x: -100.0,
                y: -100.0,
            },
        );
        let before = collapsed.clone();
        resize_curve(&mut collapsed, 2, Point { x: 100.0, y: 100.0 });
        assert_eq!(collapsed, before);
        assert!(
            curve_vertices(&collapsed)
                .unwrap()
                .iter()
                .all(|p| *p == Point { x: 100.0, y: 100.0 })
        );
        assert!(vertices(&collapsed).is_empty());
    }
    let mut flat = shape(
        "flat",
        ShapeKind::Ellipse,
        Point { x: 100.0, y: 100.0 },
        Point { x: 100.0, y: 200.0 },
    );
    resize_curve(&mut flat, 2, Point { x: 40.0, y: 50.0 });
    let bounds = curve_bounds(&flat).unwrap();
    assert_eq!(bounds.width(), 0.0);
    assert_eq!(bounds.height(), 150.0);
}

#[test]
fn erasing_splits_and_undo_restores_one_stroke() {
    let mut document = Document::new();
    document.pages[0].objects.push(BoardObject {
        id: new_id(),
        kind: ObjectKind::Stroke {
            points: vec![
                StrokePoint {
                    x: 0.0,
                    y: 10.0,
                    time: 0.0,
                    pressure: 1.0,
                },
                StrokePoint {
                    x: 100.0,
                    y: 10.0,
                    time: 1.0,
                    pressure: 1.0,
                },
            ],
            style: Style::default(),
        },
    });
    let mut history = History::new(&document);
    let page = document.current_page().id.clone();
    let ops =
        erase_operations(document.current_page(), &[Point { x: 50.0, y: 10.0 }], 5.0).unwrap();
    history.apply(&mut document, &page, 0, &ops).unwrap();
    assert_eq!(document.current_page().objects.len(), 2);
    history.undo(&mut document).unwrap();
    assert_eq!(document.current_page().objects.len(), 1);
}
#[test]
fn erase_preview_reuses_distant_operations_and_matches_full_path() {
    let mut document = Document::new();
    for (id, y) in [("first", 0.0), ("second", 100.0)] {
        document.pages[0].objects.push(BoardObject {
            id: id.into(),
            kind: ObjectKind::Stroke {
                points: vec![
                    StrokePoint {
                        x: 0.0,
                        y,
                        time: 0.0,
                        pressure: 1.0,
                    },
                    StrokePoint {
                        x: 100.0,
                        y,
                        time: 1.0,
                        pressure: 1.0,
                    },
                ],
                style: Style {
                    width: 20.0,
                    ..Style::default()
                },
            },
        });
    }
    let original = document.clone();
    let mut preview = ErasePreview::new(&document, 2.0);
    let mut points = Vec::new();
    let normalize = |operations: &[Operation]| {
        let mut operations = operations.to_vec();
        for operation in &mut operations {
            if let Operation::Add { object } = operation {
                object.id = "piece".into();
            }
        }
        serde_json::to_value(operations).unwrap()
    };
    let mut preserved = None;
    for (index, (x, y)) in [
        (50.0, 109.0),
        (150.0, 109.0),
        (150.0, -30.0),
        (50.0, 30.0),
        (50.0, -30.0),
    ]
    .into_iter()
    .enumerate()
    {
        points.push(StrokePoint {
            x,
            y,
            time: index as f64,
            pressure: 1.0,
        });
        preview.update(&points);
        let (_, operations) = preview.result.as_ref().unwrap().as_ref().unwrap();
        let path: Vec<_> = points.iter().map(|p| Point { x: p.x, y: p.y }).collect();
        let full = erase_operations(document.current_page(), &path, 2.0).unwrap();
        assert_eq!(normalize(operations), normalize(&full));
        if index == 1 {
            preserved = Some(serde_json::to_value(&preview.operations[1]).unwrap());
        } else if index > 1 {
            assert_eq!(
                preserved.as_ref().unwrap(),
                &serde_json::to_value(&preview.operations[1]).unwrap()
            );
        }
        let before = serde_json::to_value(operations).unwrap();
        preview.update(&points);
        assert_eq!(
            before,
            serde_json::to_value(&preview.result.as_ref().unwrap().as_ref().unwrap().1)
                .unwrap()
        );
    }
    assert_eq!(document, original);
    let (expected, operations) = preview.result.as_ref().unwrap().as_ref().unwrap();
    let mut history = History::new(&document);
    let page = document.current_page().id.clone();
    history.apply(&mut document, &page, 0, operations).unwrap();
    assert_eq!(&document, expected);
    history.undo(&mut document).unwrap();
    assert_eq!(document.pages, original.pages);
    assert!(!history.can_undo());
    history.redo(&mut document).unwrap();
    assert_eq!(document.pages, expected.pages);
    // A shortened path must discard operations from the removed segments.
    preview.update(&points[..1]);
    let (_, operations) = preview.result.as_ref().unwrap().as_ref().unwrap();
    let full = erase_operations(
        original.current_page(),
        &[Point {
            x: points[0].x,
            y: points[0].y,
        }],
        2.0,
    )
    .unwrap();
    assert_eq!(normalize(operations), normalize(&full));
    preview.update(&points);
    let (_, operations) = preview.result.as_ref().unwrap().as_ref().unwrap();
    let full = erase_operations(
        original.current_page(),
        &points
            .iter()
            .map(|p| Point { x: p.x, y: p.y })
            .collect::<Vec<_>>(),
        2.0,
    )
    .unwrap();
    assert_eq!(normalize(operations), normalize(&full));
}

#[test]
fn erase_broadphase_keeps_wide_strokes_and_nonstroke_point_semantics() {
    let mut document = Document::new();
    document.pages[0].objects.push(BoardObject {
        id: "wide".into(),
        kind: ObjectKind::Stroke {
            points: vec![StrokePoint {
                x: 0.0,
                y: 0.0,
                time: 0.0,
                pressure: 1.0,
            }],
            style: Style {
                width: 100.0,
                ..Style::default()
            },
        },
    });
    let operations =
        erase_operations(document.current_page(), &[Point { x: 50.5, y: 0.0 }], 1.0).unwrap();
    assert!(matches!(&operations[..], [Operation::Delete { id }] if id == "wide"));
    assert!(
        erase_operations(document.current_page(), &[], 1.0)
            .unwrap()
            .is_empty()
    );
    document.pages[0].objects = vec![shape(
        "box",
        ShapeKind::Rectangle,
        Point::default(),
        Point { x: 10.0, y: 10.0 },
    )];
    let path = [Point { x: -50.0, y: 5.0 }, Point { x: 50.0, y: 5.0 }];
    assert!(
        erase_operations(document.current_page(), &path, 1.0)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn selection_hits_lines_not_empty_bounding_box() {
    let line = shape(
        "line",
        ShapeKind::Line,
        Point::default(),
        Point { x: 100.0, y: 100.0 },
    );
    assert!(hit_test(&line, egui::pos2(50.0, 52.0), 8.0));
    assert!(!hit_test(&line, egui::pos2(10.0, 90.0), 8.0));
    let rect = shape(
        "rect",
        ShapeKind::Rectangle,
        Point::default(),
        Point { x: 100.0, y: 100.0 },
    );
    assert!(!hit_test(&rect, egui::pos2(50.0, 50.0), 8.0));
    assert!(hit_test(&rect, egui::pos2(100.0, 50.0), 8.0));
}

#[test]
fn bbox_becomes_explicit_vertices_on_edit() {
    let mut object = BoardObject {
        id: new_id(),
        kind: ObjectKind::Shape {
            shape: ShapeKind::Rectangle,
            points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 100.0, y: 50.0 }],
            style: Style::default(),
        },
    };
    move_vertex(&mut object, 1, Point { x: 120.0, y: 5.0 });
    let handles = vertices(&object);
    assert_eq!(handles.len(), 4);
    assert_eq!(handles[1], Point { x: 120.0, y: 5.0 });
    assert_eq!(handles[2], Point { x: 100.0, y: 50.0 });
}
fn shape(id: &str, kind: ShapeKind, a: Point, b: Point) -> BoardObject {
    BoardObject {
        id: id.into(),
        kind: ObjectKind::Shape {
            shape: kind,
            points: vec![a, b],
            style: Style::default(),
        },
    }
}
#[test]
fn persistent_edge_follows_moves_reconnects_and_undoes_atomically() {
    let mut document = Document::new();
    document.pages[0].objects.push(shape(
        "target",
        ShapeKind::Rectangle,
        Point::default(),
        Point { x: 100.0, y: 50.0 },
    ));
    let mut history = History::new(&document);
    let line = shape(
        "line",
        ShapeKind::Line,
        Point { x: 50.0, y: 3.0 },
        Point { x: 180.0, y: 120.0 },
    );
    commit_line(&mut document, &mut history, line, true, &[0, 1]).unwrap();
    assert_eq!(document.revision, 1);
    assert_eq!(document.connections.len(), 1);
    assert!(matches!(
        document.connections[0].target,
        board_core::Anchor::Edge {
            start: 0,
            end: 1,
            ..
        }
    ));
    assert_eq!(vertices(&document.current_page().objects[0]).len(), 4);
    let connected = document.clone();
    let mut target = document.current_page().objects[0].clone();
    translate(&mut target, Point { x: 20.0, y: 30.0 });
    let page = document.current_page().id.clone();
    history
        .apply(
            &mut document,
            &page,
            1,
            &[Operation::Update { object: target }],
        )
        .unwrap();
    assert_eq!(
        vertices(&document.current_page().objects[1])[0],
        Point { x: 70.0, y: 30.0 }
    );
    history.undo(&mut document).unwrap();
    assert_eq!(document.connections, connected.connections);
    assert_eq!(
        document.current_page().objects,
        connected.current_page().objects
    );
    let mut line = document.current_page().objects[1].clone();
    move_vertex(&mut line, 0, Point { x: 100.0, y: 50.0 });
    let revision = document.revision;
    commit_line(&mut document, &mut history, line, false, &[0]).unwrap();
    assert_eq!(document.revision, revision + 1);
    assert_eq!(document.connections.len(), 1);
    assert_eq!(
        document.connections[0].target,
        board_core::Anchor::Vertex { index: 2 }
    );
    disconnect_line(&mut document, &mut history, "line").unwrap();
    assert!(document.connections.is_empty());
    history.undo(&mut document).unwrap();
    assert_eq!(document.connections.len(), 1);
    // 序列化仍携带持久连接。
    let restored = Document::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(restored.connections, document.connections);
}
#[test]
fn mixed_dimension_transaction_rolls_back_materialization_and_line() {
    let mut document = Document::new();
    document.pages[0].objects = vec![
        shape(
            "plane",
            ShapeKind::Rectangle,
            Point::default(),
            Point { x: 100.0, y: 50.0 },
        ),
        shape(
            "solid",
            ShapeKind::Cube,
            Point { x: 300.0, y: 300.0 },
            Point { x: 400.0, y: 400.0 },
        ),
    ];
    let mut history = History::new(&document);
    let before = document.clone();
    let geometry = board_ink::shape_geometry(
        ShapeKind::Cube,
        Point { x: 300.0, y: 300.0 },
        Point { x: 400.0, y: 400.0 },
    )
    .unwrap();
    let line = shape(
        "line",
        ShapeKind::Line,
        Point::default(),
        geometry.vertices[0],
    );
    assert!(commit_line(&mut document, &mut history, line, true, &[0, 1]).is_err());
    assert_eq!(document, before);
    assert!(!history.can_undo());
}
#[test]
fn line_endpoint_cannot_close_dependency_cycle_or_partially_update() {
    let mut document = Document::new();
    document.pages[0].objects = vec![
        shape(
            "a",
            ShapeKind::Line,
            Point { x: 100.0, y: 100.0 },
            Point { x: 200.0, y: 100.0 },
        ),
        shape(
            "b",
            ShapeKind::Line,
            Point { x: 300.0, y: 300.0 },
            Point { x: 400.0, y: 300.0 },
        ),
    ];
    let page = document.current_page().id.clone();
    document
        .connect(
            &page,
            0,
            board_core::Connection {
                id: "existing".into(),
                page_id: page.clone(),
                line_id: "b".into(),
                line_endpoint: 1,
                target_id: "a".into(),
                target: board_core::Anchor::Vertex { index: 1 },
            },
        )
        .unwrap();
    let mut history = History::new(&document);
    let before = document.clone();
    let mut line = document.current_page().objects[0].clone();
    move_vertex(&mut line, 0, Point { x: 300.0, y: 300.0 });
    let error = commit_line(&mut document, &mut history, line, false, &[0]).unwrap_err();
    assert!(error.to_string().contains("依赖环"));
    assert_eq!(document, before);
    assert!(!history.can_undo());
    assert!(!history.is_dirty(&document));
}

#[test]
fn plot_zoom_preserves_center() {
    let mut object = BoardObject {
        id: new_id(),
        kind: ObjectKind::FunctionPlot {
            position: Point::default(),
            width: 300.0,
            height: 200.0,
            expressions: vec!["x".into()],
            x_min: -10.0,
            x_max: 10.0,
            y_min: -5.0,
            y_max: 5.0,
        },
    };
    scale_plot(&mut object, 0.5);
    if let ObjectKind::FunctionPlot { x_min, x_max, .. } = object.kind {
        assert_eq!((x_min, x_max), (-5.0, 5.0));
    } else {
        panic!("plot expected");
    }
}
