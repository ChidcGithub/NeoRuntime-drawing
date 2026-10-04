use super::*;

fn shape(id: &str, kind: ShapeKind, coordinates: &[(f32, f32)]) -> BoardObject {
    BoardObject {
        id: id.into(),
        kind: ObjectKind::Shape {
            shape: kind,
            points: coordinates.iter().map(|&(x, y)| Point { x, y }).collect(),
            style: Style::default(),
        },
    }
}
fn line(id: &str) -> BoardObject {
    shape(id, ShapeKind::Line, &[(0.0, 0.0), (20.0, 20.0)])
}
fn fixture() -> Document {
    let mut d = Document::new();
    d.pages[0].objects = vec![
        shape(
            "shape",
            ShapeKind::Rectangle,
            &[(2.0, 4.0), (10.0, 4.0), (10.0, 12.0), (2.0, 12.0)],
        ),
        line("a"),
        line("b"),
        line("c"),
        shape(
            "cube",
            ShapeKind::Cube,
            &[(1.0, 1.0), (5.0, 1.0), (5.0, 5.0)],
        ),
    ];
    d
}
fn connection(
    d: &Document,
    id: &str,
    line: &str,
    endpoint: usize,
    target: &str,
    anchor: Anchor,
) -> Connection {
    Connection {
        id: id.into(),
        page_id: d.pages[0].id.clone(),
        line_id: line.into(),
        line_endpoint: endpoint,
        target_id: target.into(),
        target: anchor,
    }
}
fn connect(d: &mut Document, c: Connection) -> Result<()> {
    d.connect(&d.pages[0].id.clone(), d.revision, c)
}
fn vertex(d: &mut Document, id: &str, line: &str, endpoint: usize, target: &str, index: usize) {
    let c = connection(d, id, line, endpoint, target, Anchor::Vertex { index });
    connect(d, c).unwrap();
}
fn update(d: &mut Document, object: BoardObject) -> Result<()> {
    d.apply(
        &d.pages[0].id.clone(),
        d.revision,
        &[Operation::Update { object }],
    )
}
fn points<'a>(d: &'a Document, id: &str) -> &'a [Point] {
    let object = d.pages[0].objects.iter().find(|o| o.id == id).unwrap();
    let ObjectKind::Shape { points, .. } = &object.kind else {
        panic!()
    };
    points
}

#[test]
fn connect_vertex_edge_and_transitive_following_are_atomic() {
    let mut d = fixture();
    // Deliberately insert reverse dependency order.
    vertex(&mut d, "bc", "c", 0, "b", 0);
    let edge = connection(
        &d,
        "ab",
        "b",
        0,
        "a",
        Anchor::Edge {
            start: 0,
            end: 1,
            t: 0.25,
        },
    );
    connect(&mut d, edge).unwrap();
    vertex(&mut d, "sa", "a", 0, "shape", 0);
    assert_eq!(points(&d, "a")[0], Point { x: 2.0, y: 4.0 });
    assert_eq!(points(&d, "b")[0], Point { x: 6.5, y: 8.0 });
    assert_eq!(points(&d, "c")[0], points(&d, "b")[0]);
    let revision = d.revision;
    update(
        &mut d,
        shape(
            "shape",
            ShapeKind::Rectangle,
            &[(10.0, 20.0), (30.0, 20.0), (30.0, 40.0)],
        ),
    )
    .unwrap();
    assert_eq!(d.revision, revision + 1);
    assert_eq!(points(&d, "a")[0], Point { x: 10.0, y: 20.0 });
    assert_eq!(points(&d, "b")[0], Point { x: 12.5, y: 20.0 });
    assert_eq!(points(&d, "c")[0], points(&d, "b")[0]);
    d.validate().unwrap();
}

#[test]
fn anchored_endpoint_is_authoritative_and_free_endpoint_can_move() {
    let mut d = fixture();
    vertex(&mut d, "sa", "a", 0, "shape", 0);
    let revision = d.revision;
    update(
        &mut d,
        shape("a", ShapeKind::Line, &[(999.0, 999.0), (20.0, 20.0)]),
    )
    .unwrap();
    assert_eq!(d.revision, revision);
    update(
        &mut d,
        shape("a", ShapeKind::Line, &[(999.0, 999.0), (40.0, 50.0)]),
    )
    .unwrap();
    assert_eq!(d.revision, revision + 1);
    assert_eq!(
        points(&d, "a"),
        &[Point { x: 2.0, y: 4.0 }, Point { x: 40.0, y: 50.0 }]
    );
}

#[test]
fn invalid_connections_are_rejected_without_changes() {
    let base = fixture();
    let good = connection(&base, "sa", "a", 0, "shape", Anchor::Vertex { index: 0 });
    let mut invalid = Vec::new();
    for anchor in [
        Anchor::Vertex { index: usize::MAX },
        Anchor::Edge {
            start: 0,
            end: 8,
            t: 0.5,
        },
        Anchor::Edge {
            start: 0,
            end: 0,
            t: 0.5,
        },
        Anchor::Edge {
            start: 0,
            end: 1,
            t: -0.1,
        },
        Anchor::Edge {
            start: 0,
            end: 1,
            t: 1.1,
        },
        Anchor::Edge {
            start: 0,
            end: 1,
            t: f32::NAN,
        },
        Anchor::Edge {
            start: 0,
            end: 1,
            t: f32::INFINITY,
        },
    ] {
        invalid.push(Connection {
            target: anchor,
            ..good.clone()
        });
    }
    invalid.extend([
        Connection {
            id: " ".into(),
            ..good.clone()
        },
        Connection {
            page_id: "unknown".into(),
            ..good.clone()
        },
        Connection {
            line_endpoint: 2,
            ..good.clone()
        },
        Connection {
            line_id: "shape".into(),
            target_id: "cube".into(),
            ..good.clone()
        },
        Connection {
            line_id: "missing".into(),
            ..good.clone()
        },
        Connection {
            target_id: "missing".into(),
            ..good.clone()
        },
        Connection {
            target_id: "a".into(),
            ..good.clone()
        },
    ]);
    for c in invalid {
        let mut d = base.clone();
        assert!(connect(&mut d, c).is_err());
        assert_eq!(d, base);
    }
    let mut d = base;
    connect(&mut d, good.clone()).unwrap();
    let before = d.clone();
    assert!(
        connect(
            &mut d,
            Connection {
                id: "other".into(),
                ..good.clone()
            }
        )
        .is_err()
    );
    assert_eq!(d, before);
    assert!(
        connect(
            &mut d,
            Connection {
                line_endpoint: 1,
                ..good
            }
        )
        .is_err()
    );
    assert_eq!(d, before);
}

#[test]
fn cycles_and_undirected_mixed_dimension_components_are_rejected() {
    let mut d = fixture();
    vertex(&mut d, "ab", "a", 0, "b", 0);
    vertex(&mut d, "bc", "b", 0, "c", 0);
    let before = d.clone();
    let c = connection(&d, "ca", "c", 0, "a", Anchor::Vertex { index: 0 });
    assert!(connect(&mut d, c).is_err());
    assert_eq!(d, before);
    vertex(&mut d, "sa", "a", 1, "shape", 0);
    let before = d.clone();
    let c = connection(&d, "cube-c", "c", 0, "cube", Anchor::Vertex { index: 0 });
    assert!(connect(&mut d, c).is_err());
    assert_eq!(d, before);
    let page = d.pages[0].id.clone();
    d.disconnect(&page, d.revision, "sa").unwrap();
    vertex(&mut d, "cube-c", "c", 0, "cube", 0);
    d.validate().unwrap();
}

#[test]
fn dimension_changes_and_vertex_removal_rollback_whole_batch() {
    let mut d = fixture();
    vertex(&mut d, "sa", "a", 0, "shape", 3);
    vertex(&mut d, "ba", "b", 0, "a", 0);
    vertex(&mut d, "sc", "b", 1, "shape", 0);
    // Another 2D target anchors the same component.
    d.pages[0].objects.push(shape(
        "triangle",
        ShapeKind::Triangle,
        &[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0)],
    ));
    vertex(&mut d, "tc", "c", 0, "triangle", 0);
    vertex(&mut d, "cb", "c", 1, "b", 1);
    let before = d.clone();
    for object in [
        shape(
            "shape",
            ShapeKind::Rectangle,
            &[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0)],
        ),
        shape(
            "shape",
            ShapeKind::Cube,
            &[(2.0, 4.0), (10.0, 4.0), (10.0, 12.0), (2.0, 12.0)],
        ),
        shape(
            "a",
            ShapeKind::Triangle,
            &[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0)],
        ),
    ] {
        assert!(
            d.apply(
                &before.pages[0].id,
                d.revision,
                &[
                    Operation::Add {
                        object: line("new")
                    },
                    Operation::Update { object }
                ]
            )
            .is_err()
        );
        assert_eq!(d, before);
    }
}

#[test]
fn rejects_cross_page_nonshape_bbox_and_nonfinite_vertices() {
    for object in [
        shape("shape", ShapeKind::Rectangle, &[(0.0, 0.0), (10.0, 10.0)]),
        shape(
            "shape",
            ShapeKind::Line,
            &[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)],
        ),
        BoardObject {
            id: "shape".into(),
            kind: ObjectKind::CoordinateSystem {
                origin: Point::default(),
                scale: 1.0,
            },
        },
        shape(
            "shape",
            ShapeKind::Rectangle,
            &[(0.0, 0.0), (10.0, 10.0), (f32::NAN, 0.0)],
        ),
    ] {
        let mut d = fixture();
        d.pages[0].objects[0] = object;
        let c = connection(&d, "sa", "a", 0, "shape", Anchor::Vertex { index: 0 });
        assert!(connect(&mut d, c).is_err());
        assert!(d.connections.is_empty());
    }
    let mut d = fixture();
    let target = d.pages[0].objects.remove(0);
    d.add_page().unwrap();
    d.pages[1].objects.push(target);
    let c = connection(&d, "sa", "a", 0, "shape", Anchor::Vertex { index: 0 });
    let before = d.clone();
    assert!(connect(&mut d, c).is_err());
    assert_eq!(d, before);
}

#[test]
fn delete_object_page_and_delete_readd_remove_connections() {
    for id in ["shape", "a"] {
        let mut d = fixture();
        vertex(&mut d, "sa", "a", 0, "shape", 0);
        vertex(&mut d, "ab", "b", 0, "a", 0);
        let page = d.pages[0].id.clone();
        let revision = d.revision;
        d.apply(&page, revision, &[Operation::Delete { id: id.into() }])
            .unwrap();
        assert_eq!(d.revision, revision + 1);
        assert!(
            !d.connections
                .iter()
                .any(|c| c.line_id == id || c.target_id == id)
        );
        d.validate().unwrap();
    }
    let mut d = fixture();
    vertex(&mut d, "sa", "a", 0, "shape", 0);
    let page = d.pages[0].id.clone();
    d.apply(
        &page,
        d.revision,
        &[
            Operation::Delete { id: "a".into() },
            Operation::Add { object: line("a") },
        ],
    )
    .unwrap();
    assert!(d.connections.is_empty());
    vertex(&mut d, "sa", "a", 0, "shape", 0);
    d.add_page().unwrap();
    let mut history = History::new(&d);
    history.edit(&mut d, |d| d.delete_page(&page)).unwrap();
    assert!(d.connections.is_empty());
    history.undo(&mut d).unwrap();
    assert_eq!(d.connections.len(), 1);
    d.validate().unwrap();
}

#[test]
fn disconnect_history_dirty_save_undo_redo_and_divergence_include_connections() {
    let mut d = fixture();
    // Endpoints already coincide: connection-only edit must still be dirty.
    if let ObjectKind::Shape { points, .. } = &mut d.pages[0].objects[1].kind {
        points[0] = Point { x: 2.0, y: 4.0 };
    }
    let mut history = History::new(&d);
    let c = connection(&d, "sa", "a", 0, "shape", Anchor::Vertex { index: 0 });
    let page = d.pages[0].id.clone();
    history
        .edit(&mut d, |d| d.connect(&page, d.revision, c))
        .unwrap();
    assert_eq!(d.revision, 1);
    assert!(history.is_dirty(&d));
    history.undo(&mut d).unwrap();
    assert_eq!(d.revision, 2);
    assert!(!history.is_dirty(&d));
    history.redo(&mut d).unwrap();
    history.mark_saved(&d);
    let before_points = points(&d, "a").to_vec();
    history
        .edit(&mut d, |d| d.disconnect(&page, d.revision, "sa"))
        .unwrap();
    assert!(history.is_dirty(&d));
    assert_eq!(points(&d, "a"), before_points);
    history.undo(&mut d).unwrap();
    assert!(!history.is_dirty(&d));
    d.connections.clear();
    assert!(matches!(history.undo(&mut d), Err(Error::HistoryDiverged)));
}

#[test]
fn propagation_and_deletion_are_single_undoable_edits() {
    let mut d = fixture();
    vertex(&mut d, "sa", "a", 0, "shape", 0);
    vertex(&mut d, "ab", "b", 0, "a", 0);
    let before = d.clone();
    let page = d.pages[0].id.clone();
    let mut history = History::new(&d);
    history
        .edit(&mut d, |d| {
            update(
                d,
                shape(
                    "shape",
                    ShapeKind::Rectangle,
                    &[(7.0, 8.0), (20.0, 8.0), (20.0, 30.0)],
                ),
            )?;
            d.disconnect(&page, d.revision, "ab")
        })
        .unwrap();
    assert_eq!(d.revision, before.revision + 1);
    assert_eq!(points(&d, "b")[0], Point { x: 7.0, y: 8.0 });
    let after = d.clone();
    history.undo(&mut d).unwrap();
    assert!(same_content(&d, &before));
    history.redo(&mut d).unwrap();
    assert!(same_content(&d, &after));
    history
        .apply(
            &mut d,
            &page,
            after.revision + 2,
            &[Operation::Delete { id: "shape".into() }],
        )
        .unwrap();
    assert!(d.connections.is_empty());
    history.undo(&mut d).unwrap();
    assert!(same_content(&d, &after));
}

#[test]
fn connect_disconnect_revision_conflict_missing_id_and_overflow_are_atomic() {
    let mut d = fixture();
    let page = d.pages[0].id.clone();
    let c = connection(&d, "sa", "a", 0, "shape", Anchor::Vertex { index: 0 });
    let before = d.clone();
    assert!(matches!(
        d.connect(&page, 1, c.clone()),
        Err(Error::RevisionConflict { .. })
    ));
    assert!(d.connect("unknown", 0, c.clone()).is_err());
    assert!(d.disconnect(&page, 0, "missing").is_err());
    assert_eq!(d, before);
    d.revision = u64::MAX;
    let before = d.clone();
    assert!(matches!(
        connect(&mut d, c.clone()),
        Err(Error::RevisionOverflow)
    ));
    assert_eq!(d, before);
    d.revision = 0;
    connect(&mut d, c).unwrap();
    let before = d.clone();
    assert!(d.disconnect(&page, 0, "sa").is_err());
    assert!(d.disconnect("missing", d.revision, "sa").is_err());
    assert_eq!(d, before);
    d.revision = u64::MAX;
    let before = d.clone();
    assert!(matches!(
        d.disconnect(&page, d.revision, "sa"),
        Err(Error::RevisionOverflow)
    ));
    assert_eq!(d, before);
}

#[test]
fn serde_and_disk_roundtrip_and_old_files_without_connections() {
    let mut d = fixture();
    let c = connection(
        &d,
        "sa",
        "a",
        1,
        "shape",
        Anchor::Edge {
            start: 0,
            end: 3,
            t: 0.25,
        },
    );
    let value = serde_json::to_value(&c).unwrap();
    assert_eq!(
        value["target"],
        serde_json::json!({"type":"edge", "start":0, "end":3, "t":0.25})
    );
    connect(&mut d, c).unwrap();
    assert_eq!(Document::from_json(&d.to_json().unwrap()).unwrap(), d);
    let path = std::env::temp_dir().join(format!("board-connection-{}.json", new_id()));
    let mut history = History::new(&fixture());
    history.save(&d, &path).unwrap();
    assert!(!history.is_dirty(&d));
    assert_eq!(Document::load(&path).unwrap(), d);
    fs::remove_file(path).unwrap();
    let mut value = serde_json::to_value(&d).unwrap();
    value.as_object_mut().unwrap().remove("connections");
    let legacy: Document = serde_json::from_value(value).unwrap();
    assert!(legacy.connections.is_empty());
}

#[test]
fn untrusted_serde_documents_cannot_bypass_connection_validation() {
    let mut d = fixture();
    vertex(&mut d, "sa", "a", 0, "shape", 0);
    let good = serde_json::to_value(&d).unwrap();
    let mut invalid = Vec::new();
    for (key, value) in [
        ("id", serde_json::json!("")),
        ("page_id", serde_json::json!("bad")),
        ("line_endpoint", serde_json::json!(2)),
        ("target_id", serde_json::json!("a")),
        ("target", serde_json::json!({"type":"vertex", "index":99})),
        (
            "target",
            serde_json::json!({"type":"edge", "start":0,"end":1,"t":2.0}),
        ),
        ("surprise", serde_json::json!(1)),
    ] {
        let mut bad = good.clone();
        bad["connections"][0][key] = value;
        invalid.push(bad);
    }
    let mut bad = good.clone();
    bad["pages"][0]["objects"][1]["kind"]["points"][0]["x"] = serde_json::json!(99.0);
    invalid.push(bad);
    let mut bad = good.clone();
    bad["connections"]
        .as_array_mut()
        .unwrap()
        .push(good["connections"][0].clone());
    invalid.push(bad);
    let mut bad = good.clone();
    let mut extra = good["connections"][0].clone();
    extra["id"] = serde_json::json!("cube-a");
    extra["line_endpoint"] = serde_json::json!(1);
    extra["target_id"] = serde_json::json!("cube");
    bad["connections"].as_array_mut().unwrap().push(extra);
    bad["pages"][0]["objects"][1]["kind"]["points"][1] = serde_json::json!({"x":1.0,"y":1.0});
    invalid.push(bad);
    for bad in invalid {
        assert!(serde_json::from_value::<Document>(bad.clone()).is_err());
        assert!(
            Document::from_json(&serde_json::json!({"version":1,"document":bad}).to_string())
                .is_err()
        );
    }
}

#[test]
fn extreme_finite_interpolation_and_degenerate_segments_stay_finite() {
    let mut d = fixture();
    d.pages[0].objects[0] = shape(
        "shape",
        ShapeKind::Triangle,
        &[
            (-f32::MAX, f32::MAX),
            (f32::MAX, -f32::MAX),
            (f32::MAX, -f32::MAX),
        ],
    );
    for (i, t) in [0.0, 0.5, 1.0].into_iter().enumerate() {
        let c = connection(
            &d,
            &i.to_string(),
            ["a", "b", "c"][i],
            0,
            "shape",
            Anchor::Edge {
                start: 0,
                end: 1,
                t,
            },
        );
        connect(&mut d, c).unwrap();
    }
    assert_eq!(points(&d, "b")[0], Point::default());
    assert_eq!(points(&d, "a")[0].x, -f32::MAX);
    assert_eq!(points(&d, "c")[0].x, f32::MAX);
    let c = connection(
        &d,
        "degenerate",
        "a",
        1,
        "shape",
        Anchor::Edge {
            start: 1,
            end: 2,
            t: 0.4,
        },
    );
    connect(&mut d, c).unwrap();
    d.validate().unwrap();
}

#[test]
fn long_reverse_order_dag_is_iterative_and_linear() {
    let mut d = Document::new();
    let count = 12_000;
    for i in 0..count {
        d.pages[0].objects.push(line(&i.to_string()));
    }
    for i in (1..count).rev() {
        d.connections.push(connection(
            &d,
            &format!("c{i}"),
            &i.to_string(),
            0,
            &(i - 1).to_string(),
            Anchor::Vertex { index: 0 },
        ));
    }
    d.validate().unwrap();
    update(
        &mut d,
        shape("0", ShapeKind::Line, &[(9.0, 7.0), (20.0, 20.0)]),
    )
    .unwrap();
    assert_eq!(d.revision, 1);
    assert_eq!(
        points(&d, &(count - 1).to_string())[0],
        Point { x: 9.0, y: 7.0 }
    );
    d.connections.push(connection(
        &d,
        "cycle",
        "0",
        0,
        &(count - 1).to_string(),
        Anchor::Vertex { index: 0 },
    ));
    assert!(d.validate().is_err());
}

#[test]
fn budgets_reject_oversized_points_objects_connections_and_batches() {
    let mut d = fixture();
    let before = d.clone();
    let operations = vec![Operation::Delete { id: "a".into() }; MAX_OPERATIONS + 1];
    assert!(d.apply(&before.pages[0].id, 0, &operations).is_err());
    assert_eq!(d, before);
    let mut object = line("a");
    if let ObjectKind::Shape { points, .. } = &mut object.kind {
        *points = vec![Point::default(); MAX_DOCUMENT_POINTS + 1];
    }
    assert!(update(&mut d, object).is_err());
    assert_eq!(d, before);
    d.pages[0].objects = vec![line("a"); MAX_DOCUMENT_OBJECTS + 1];
    assert!(d.validate().is_err());
    d = before.clone();
    d.connections = vec![
        connection(&d, "c", "a", 0, "shape", Anchor::Vertex { index: 0 });
        MAX_CONNECTIONS + 1
    ];
    assert!(d.validate().is_err());
    d = before;
    d.pages[0].objects[0] = BoardObject {
        id: "large".into(),
        kind: ObjectKind::Text {
            position: Point::default(),
            text: "x".repeat(MAX_DOCUMENT_BYTES + 1),
            size: 10.0,
            color: Color::default(),
        },
    };
    assert!(d.validate().is_err());
}

#[test]
fn five_hundred_nonempty_pages_and_large_batches_preserve_order() {
    let mut d = Document::new();
    d.pages = (0..MAX_PAGES)
        .map(|i| Page {
            id: format!("page-{i}"),
            objects: vec![line(&format!("object-{i}"))],
        })
        .collect();
    d.validate().unwrap();
    let page = d.pages[0].id.clone();
    let operations: Vec<_> = (0..MAX_OPERATIONS)
        .map(|i| Operation::Add {
            object: line(&format!("added-{i}")),
        })
        .collect();
    d.apply(&page, 0, &operations).unwrap();
    let operations: Vec<_> = (0..MAX_OPERATIONS)
        .rev()
        .map(|i| Operation::Delete {
            id: format!("added-{i}"),
        })
        .collect();
    d.apply(&page, 1, &operations).unwrap();
    assert_eq!(d.pages[0].objects, vec![line("object-0")]);
    assert_eq!(d.pages.len(), MAX_PAGES);
}

#[test]
fn history_budget_bounds_snapshots_and_preserves_saved_baseline() {
    let mut d = fixture();
    let mut history = History::new(&d);
    for i in 0..MAX_HISTORY_ENTRIES + 5 {
        history
            .edit(&mut d, |d| {
                update(
                    d,
                    shape("a", ShapeKind::Line, &[(i as f32, 3.0), (20.0, 20.0)]),
                )
            })
            .unwrap();
    }
    assert_eq!(history.undo.len(), MAX_HISTORY_ENTRIES);
    history.mark_saved(&d);
    history.undo(&mut d).unwrap();
    assert!(history.is_dirty(&d));
    history.redo(&mut d).unwrap();
    assert!(!history.is_dirty(&d));
    let mut d = Document::new();
    d.pages[0].objects.push(BoardObject {
        id: "text".into(),
        kind: ObjectKind::Text {
            position: Point::default(),
            text: "x".repeat(9 * 1024 * 1024),
            size: 10.0,
            color: Color::default(),
        },
    });
    let mut history = History::new(&d);
    for _ in 0..10 {
        history.edit(&mut d, |d| d.add_page()).unwrap();
    }
    assert!(history.undo.len() < 10);
    assert!(history.undo.iter().map(|(_, bytes)| bytes).sum::<usize>() <= MAX_HISTORY_BYTES);
}

#[test]
fn materialize_and_connect_share_one_history_revision_and_rollback() {
    let mut d = fixture();
    d.pages[0].objects[0] = shape("shape", ShapeKind::Rectangle, &[(2.0, 4.0), (10.0, 12.0)]);
    let before = d.clone();
    let page = d.pages[0].id.clone();
    let mut history = History::new(&d);
    let c = connection(&d, "sa", "a", 0, "shape", Anchor::Vertex { index: 3 });
    history
        .edit(&mut d, |d| {
            update(
                d,
                shape(
                    "shape",
                    ShapeKind::Rectangle,
                    &[(2.0, 4.0), (10.0, 4.0), (10.0, 12.0), (2.0, 12.0)],
                ),
            )?;
            d.connect(&page, d.revision, c)
        })
        .unwrap();
    assert_eq!(d.revision, 1);
    assert_eq!(points(&d, "a")[0], Point { x: 2.0, y: 12.0 });
    history.undo(&mut d).unwrap();
    assert!(same_content(&d, &before));
    let before = d.clone();
    let c = connection(&d, "sa", "a", 0, "shape", Anchor::Vertex { index: 9 });
    assert!(
        history
            .edit(&mut d, |d| {
                update(
                    d,
                    shape(
                        "shape",
                        ShapeKind::Rectangle,
                        &[(2.0, 4.0), (10.0, 4.0), (10.0, 12.0), (2.0, 12.0)],
                    ),
                )?;
                d.connect(&page, d.revision, c)
            })
            .is_err()
    );
    assert_eq!(d, before);
    assert!(history.can_redo());
}

#[test]
fn both_endpoints_on_same_target_and_downstream_wait_for_both_updates() {
    let mut d = fixture();
    vertex(&mut d, "a0", "a", 0, "shape", 0);
    vertex(&mut d, "a1", "a", 1, "shape", 3);
    let c = connection(
        &d,
        "ab",
        "b",
        0,
        "a",
        Anchor::Edge {
            start: 0,
            end: 1,
            t: 0.5,
        },
    );
    connect(&mut d, c).unwrap();
    update(
        &mut d,
        shape(
            "shape",
            ShapeKind::Rectangle,
            &[(20.0, 30.0), (50.0, 30.0), (50.0, 70.0), (20.0, 70.0)],
        ),
    )
    .unwrap();
    assert_eq!(points(&d, "b")[0], Point { x: 20.0, y: 50.0 });
    d.validate().unwrap();
}

#[test]
fn oversized_disk_input_is_rejected_before_reading() {
    let path = std::env::temp_dir().join(format!("board-large-{}.json", new_id()));
    let file = fs::File::create(&path).unwrap();
    file.set_len(MAX_JSON_BYTES as u64 + 1).unwrap();
    drop(file);
    assert!(matches!(
        Document::load(&path),
        Err(Error::InvalidDocument(_))
    ));
    fs::remove_file(path).unwrap();
}
