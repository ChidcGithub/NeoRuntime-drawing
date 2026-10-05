use super::*;

// #region debug-point B:history-microbench
thread_local! {
    static HISTORY_PROBES: std::cell::RefCell<Option<std::collections::BTreeMap<&'static str, (u64, f64)>>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn history_probe(start: &mut std::time::Instant, stage: &'static str) {
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    HISTORY_PROBES.with_borrow_mut(|probes| {
        if let Some(probes) = probes {
            let entry = probes.entry(stage).or_default();
            entry.0 += 1;
            entry.1 += elapsed;
        }
    });
    *start = std::time::Instant::now();
}

fn history_bench_stroke(id: &str) -> BoardObject {
    let mut object = stroke(id);
    if let ObjectKind::Stroke { points, .. } = &mut object.kind {
        *points = (0..128)
            .map(|i| StrokePoint {
                x: i as f32,
                y: (i % 13) as f32,
                time: i as f64 / 1000.0,
                pressure: 0.5,
            })
            .collect();
    }
    object
}

#[test]
#[ignore = "short release history phase benchmark; reports to stderr"]
fn history_phase_microbench() {
    let run = std::env::var("HISTORY_BENCH_RUN_ID").unwrap_or_else(|_| "pre-history".into());
    for count in [100, 500, 1500] {
        for scenario in ["continuous_add", "erase_batch", "new_page", "failed_batch"] {
            let mut document = Document::new();
            document.pages[0].objects = (0..count)
                .map(|i| history_bench_stroke(&format!("stroke-{i}")))
                .collect();
            document.validate().unwrap();
            let mut history = History::new(&document);
            HISTORY_PROBES.with_borrow_mut(|probes| *probes = Some(Default::default()));
            let mut total = 0.0;
            for step in 0..8 {
                let operations = match scenario {
                    "continuous_add" => vec![Operation::Add {
                        object: history_bench_stroke(&format!("new-{step}")),
                    }],
                    "erase_batch" => (0..4)
                        .map(|i| Operation::Delete {
                            id: format!("stroke-{}", step * 4 + i),
                        })
                        .collect(),
                    "failed_batch" => vec![
                        add("temporary"),
                        Operation::Delete {
                            id: "missing".into(),
                        },
                    ],
                    _ => Vec::new(),
                };
                let before = document.clone();
                let started = std::time::Instant::now();
                if scenario == "new_page" {
                    history.edit(&mut document, Document::add_page).unwrap();
                } else {
                    let result = history_apply(&mut history, &mut document, &operations);
                    if scenario == "failed_batch" {
                        assert!(result.is_err());
                    } else {
                        result.unwrap();
                    }
                }
                total += started.elapsed().as_secs_f64() * 1000.0;
                if scenario == "failed_batch" {
                    assert_eq!(document, before);
                    assert!(!history.can_undo());
                }
            }
            let phases = HISTORY_PROBES.with_borrow_mut(|probes| probes.take().unwrap());
            let phases: std::collections::BTreeMap<_, _> = phases
                .into_iter()
                .map(|(key, (calls, ms))| {
                    (
                        key,
                        serde_json::json!({"calls":calls,"mean_ms":ms / calls as f64}),
                    )
                })
                .collect();
            let body = serde_json::json!({"sessionId":"fullscreen-ink-performance","runId":run,"hypothesisId":"B","location":"board-core::tests::history_phase_microbench","msg":"[DEBUG] history phase microbench","data":{"strokes":count,"points_per_stroke":128,"scenario":scenario,"iterations":8,"mean_ms":total/8.0,"phases":phases}}).to_string();
            eprintln!("{body}");
        }
    }
}
// #endregion

fn handwritten_object() -> BoardObject {
    BoardObject {
        id: "answer".into(),
        kind: ObjectKind::Handwritten {
            position: Point { x: 20.0, y: 30.0 },
            text: "原始答案 1/2".into(),
            layout: Some(MathLayout::Fraction(
                Box::new(MathLayout::Text("1".into())),
                Box::new(MathLayout::Text("2".into())),
            )),
            strokes: vec![
                HandwritingStroke {
                    points: vec![
                        StrokePoint {
                            x: -2.0,
                            y: 3.0,
                            time: 0.25,
                            pressure: 0.3
                        },
                        StrokePoint {
                            x: 12.0,
                            y: 8.0,
                            time: 0.75,
                            pressure: 0.9
                        },
                    ],
                    style: Style::default(),
                };
                2
            ],
        },
    }
}

#[test]
fn handwritten_roundtrip_versions_and_whole_answer_history() {
    let mut document = Document::new();
    let mut history = History::new(&document);
    for layout_present in [true, false] {
        let mut object = handwritten_object();
        if !layout_present && let ObjectKind::Handwritten { layout, .. } = &mut object.kind {
            *layout = None;
        }
        history_apply(
            &mut history,
            &mut document,
            &[Operation::Add {
                object: object.clone(),
            }],
        )
        .unwrap();
        let json = document.to_json().unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["version"], 3);
        assert_eq!(
            value["document"]["pages"][0]["objects"][0]["kind"]["type"],
            "handwritten"
        );
        assert_eq!(Document::from_json(&json).unwrap(), document);
        for version in [1, 2] {
            value["version"] = serde_json::json!(version);
            assert!(matches!(
                Document::from_json(&value.to_string()),
                Err(Error::InvalidDocument(_))
            ));
        }
        let dir = TestDirectory::new();
        let path = dir.0.join("answer.json");
        document.save(&path).unwrap();
        assert_eq!(Document::load(path).unwrap(), document);
        history.undo(&mut document).unwrap();
        assert!(document.current_page().objects.is_empty());
        assert!(!history.can_undo());
        assert_eq!(document.file_version(), 1);
        history.redo(&mut document).unwrap();
        assert_eq!(document.current_page().objects, vec![object]);
        assert_eq!(document.file_version(), 3);
        history.undo(&mut document).unwrap();
    }
    document.pages[0]
        .objects
        .push(math_object(MathLayout::Text("2".into())));
    assert_eq!(document.file_version(), 2);
    document.pages[0].objects.push(handwritten_object());
    assert_eq!(document.file_version(), 3);
}

#[test]
fn handwritten_limits_and_invalid_data_are_atomic() {
    let mut boundary = handwritten_object();
    if let ObjectKind::Handwritten { text, strokes, .. } = &mut boundary.kind {
        *text = "中".repeat(MAX_HANDWRITING_TEXT_BYTES / 3) + "x";
        strokes.resize(MAX_HANDWRITING_STROKES, strokes[0].clone());
        for stroke in strokes {
            stroke.points.resize(
                MAX_HANDWRITING_POINTS / MAX_HANDWRITING_STROKES,
                stroke.points[0],
            );
        }
    }
    boundary.validate().unwrap();
    for case in 0..23 {
        let mut object = boundary.clone();
        let ObjectKind::Handwritten {
            position,
            text,
            layout,
            strokes,
        } = &mut object.kind
        else {
            unreachable!()
        };
        match case {
            0 => text.push('x'),
            1 => strokes.push(strokes[0].clone()),
            2 => {
                let point = strokes[0].points[0];
                strokes[0].points.push(point);
            }
            3 => strokes.clear(),
            4 => strokes[0].points.clear(),
            5 => position.x = f32::NAN,
            6 => position.y = f32::INFINITY,
            7 => position.x = MAX_HANDWRITING_COORD + 1.0,
            8 => {
                position.x = MAX_HANDWRITING_COORD;
                strokes[0].points[0].x = 1.0;
            }
            9 => strokes[0].points[0].x = f32::INFINITY,
            10 => strokes[0].points[0].y = -MAX_HANDWRITING_COORD - 1.0,
            11 => strokes[0].points[0].time = f64::NAN,
            12 => strokes[0].points[0].time = f64::INFINITY,
            13 => strokes[0].points[0].time = -0.1,
            14 => strokes[0].points[0].time = MAX_HANDWRITING_TIME + 1.0,
            15 => strokes[0].points[0].pressure = f32::NAN,
            16 => strokes[0].points[0].pressure = -0.1,
            17 => strokes[0].points[0].pressure = 1.1,
            18 => strokes[0].style.width = f32::NAN,
            19 => strokes[0].style.width = 0.0,
            20 => strokes[0].style.width = MAX_BRUSH_WIDTH + 1.0,
            21 => *layout = Some(MathLayout::Text("x".repeat(MAX_MATH_TEXT_BYTES + 1))),
            22 => {
                *layout = Some(MathLayout::Row(vec![
                    MathLayout::Text(String::new());
                    MAX_MATH_NODES
                ]))
            }
            _ => unreachable!(),
        }
        assert!(object.validate().is_err(), "case {case}");
        let mut document = Document::new();
        let before = document.clone();
        assert!(apply(&mut document, &[add("valid"), Operation::Add { object }]).is_err());
        assert_eq!(document, before);
    }
    let mut object = handwritten_object();
    if let ObjectKind::Handwritten { layout, .. } = &mut object.kind {
        let mut deep = MathLayout::Text("x".into());
        for _ in 0..MAX_MATH_DEPTH {
            deep = MathLayout::Radical(Box::new(deep));
        }
        *layout = Some(deep);
    }
    assert!(object.validate().is_err());
}

#[test]
fn handwritten_nested_data_counts_towards_document_and_history_budgets() {
    let mut document = Document::new();
    let before = connections::document_bytes(&document);
    let object = handwritten_object();
    document.pages[0].objects.push(object.clone());
    let bytes = connections::document_bytes(&document) - before;
    let ObjectKind::Handwritten { text, strokes, .. } = &object.kind else {
        unreachable!()
    };
    assert_eq!(
        bytes,
        std::mem::size_of::<BoardObject>()
            + object.id.len()
            + text.len()
            + strokes.len() * std::mem::size_of::<HandwritingStroke>()
            + 4 * std::mem::size_of::<StrokePoint>()
            + 3 * std::mem::size_of::<MathLayout>()
            + 2
    );
    let mut budget = connections::Budget::default();
    budget.bytes(MAX_DOCUMENT_BYTES - bytes).unwrap();
    budget.object(&object).unwrap();
    assert!(budget.bytes(1).is_err());
    let mut history = History::new(&document);
    history_apply(&mut history, &mut document, &[add("next")]).unwrap();
    assert_eq!(history.undo[0].1, before + bytes);
    history.undo(&mut document).unwrap();
    assert_eq!(
        history.redo[0].1,
        connections::document_bytes(&history.redo[0].0)
    );

    let mut large = handwritten_object();
    if let ObjectKind::Handwritten { strokes, .. } = &mut large.kind {
        strokes.truncate(1);
        strokes[0]
            .points
            .resize(MAX_HANDWRITING_POINTS, StrokePoint::default());
    }
    let mut budget = connections::Budget::default();
    for _ in 0..MAX_DOCUMENT_POINTS / MAX_HANDWRITING_POINTS {
        budget.object(&large).unwrap();
    }
    assert!(
        matches!(budget.object(&large), Err(Error::InvalidDocument(reason)) if reason.contains("点数"))
    );
}

fn math_object(layout: MathLayout) -> BoardObject {
    BoardObject {
        id: "math".into(),
        kind: ObjectKind::Math {
            position: Point { x: 10.0, y: 20.0 },
            layout,
            size: 24.0,
            color: Color::default(),
        },
    }
}

#[test]
fn math_layout_tagged_roundtrip_versions_and_single_history_entry() {
    let layout = MathLayout::Row(vec![
        MathLayout::Text("= -(".into()),
        MathLayout::Fraction(
            Box::new(MathLayout::Radical(Box::new(MathLayout::Text("2".into())))),
            Box::new(MathLayout::Text("3".into())),
        ),
        MathLayout::Text(")".into()),
    ]);
    let value = serde_json::to_value(&layout).unwrap();
    assert_eq!(value["type"], "row");
    assert_eq!(value["value"][1]["type"], "fraction");
    assert_eq!(serde_json::from_value::<MathLayout>(value).unwrap(), layout);
    let mut document = Document::new();
    let old_json = document.to_json().unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&old_json).unwrap()["version"],
        1
    );
    assert_eq!(Document::from_json(&old_json).unwrap(), document);
    let mut history = History::new(&document);
    let object = math_object(layout);
    history_apply(
        &mut history,
        &mut document,
        &[Operation::Add {
            object: object.clone(),
        }],
    )
    .unwrap();
    assert_eq!(document.current_page().objects, vec![object]);
    let json = document.to_json().unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["version"], 2);
    assert_eq!(Document::from_json(&json).unwrap(), document);
    value["version"] = serde_json::json!(1);
    assert!(Document::from_json(&value.to_string()).is_err());
    let dir = TestDirectory::new();
    let path = dir.0.join("math.json");
    document.save(&path).unwrap();
    assert_eq!(Document::load(path).unwrap(), document);
    history.undo(&mut document).unwrap();
    assert!(document.current_page().objects.is_empty());
    assert!(!history.can_undo());
    assert_eq!(document.file_version(), 1);
    history.redo(&mut document).unwrap();
    assert_eq!(document.file_version(), 2);
    assert_eq!(document.current_page().objects.len(), 1);
}

#[test]
fn math_layout_budget_boundaries_and_invalid_geometry_are_atomic() {
    let mut deepest = MathLayout::Text("x".into());
    for _ in 1..MAX_MATH_DEPTH {
        deepest = MathLayout::Radical(Box::new(deepest));
    }
    math_object(deepest.clone()).validate().unwrap();
    let mut document = Document::new();
    document.pages[0].objects.push(math_object(deepest.clone()));
    assert_eq!(
        Document::from_json(&document.to_json().unwrap()).unwrap(),
        document
    );
    math_object(MathLayout::Row(vec![
        MathLayout::Text(String::new());
        MAX_MATH_NODES - 1
    ]))
    .validate()
    .unwrap();
    math_object(MathLayout::Text("a".repeat(MAX_MATH_TEXT_BYTES)))
        .validate()
        .unwrap();
    let invalid = [
        MathLayout::Radical(Box::new(deepest)),
        MathLayout::Row(vec![MathLayout::Text(String::new()); MAX_MATH_NODES]),
        MathLayout::Row(vec![MathLayout::Text("中".repeat(700)); 2]),
    ];
    for layout in invalid {
        let mut document = Document::new();
        let before = document.clone();
        assert!(
            apply(
                &mut document,
                &[
                    add("valid"),
                    Operation::Add {
                        object: math_object(layout)
                    }
                ]
            )
            .is_err()
        );
        assert_eq!(document, before);
    }
    for size in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let mut object = math_object(MathLayout::Text("1/2".into()));
        if let ObjectKind::Math { size: value, .. } = &mut object.kind {
            *value = size;
        }
        assert!(object.validate().is_err());
    }
    for coordinate in [f32::NAN, f32::INFINITY] {
        let mut object = math_object(MathLayout::Text("1/2".into()));
        if let ObjectKind::Math { position, .. } = &mut object.kind {
            position.x = coordinate;
        }
        assert!(object.validate().is_err());
    }
}

#[test]
fn math_nodes_and_strings_count_towards_document_and_history_memory() {
    let mut document = Document::new();
    let before = connections::document_bytes(&document);
    let layout = MathLayout::Row(vec![MathLayout::Text("a".repeat(8)); 511]);
    let object = math_object(layout);
    document.pages[0].objects.push(object.clone());
    let object_bytes = connections::document_bytes(&document) - before;
    assert!(object_bytes >= 512 * std::mem::size_of::<MathLayout>() + 4088);
    let mut budget = connections::Budget::default();
    budget.bytes(MAX_DOCUMENT_BYTES - object_bytes + 1).unwrap();
    assert!(budget.object(&object).is_err());
    let mut history = History::new(&document);
    history_apply(&mut history, &mut document, &[add("another")]).unwrap();
    assert_eq!(history.undo[0].1, before + object_bytes);
}

fn stroke(id: &str) -> BoardObject {
    BoardObject {
        id: id.into(),
        kind: ObjectKind::Stroke {
            points: vec![StrokePoint {
                x: 1.0,
                y: 2.0,
                time: 0.0,
                pressure: 0.5,
            }],
            style: Style::default(),
        },
    }
}
fn add(id: &str) -> Operation {
    Operation::Add { object: stroke(id) }
}
fn apply(document: &mut Document, operations: &[Operation]) -> Result<()> {
    document.apply(
        &document.current_page().id.clone(),
        document.revision,
        operations,
    )
}
fn history_apply(
    history: &mut History,
    document: &mut Document,
    operations: &[Operation],
) -> Result<()> {
    history.apply(
        document,
        &document.current_page().id.clone(),
        document.revision,
        operations,
    )
}

#[test]
fn new_document_has_one_valid_page_and_unique_ids() {
    let a = Document::new();
    let b = Document::new();
    a.validate().unwrap();
    assert_eq!(a.pages.len(), 1);
    assert_eq!(a.revision, 0);
    assert_eq!(a.current_page(), &a.pages[0]);
    assert_ne!(a.id, b.id);
    assert_ne!(a.pages[0].id, b.pages[0].id);
}

#[test]
fn operation_and_object_tags_round_trip() {
    let value = serde_json::to_value(add("a")).unwrap();
    assert_eq!(value["op"], "add");
    assert_eq!(value["object"]["kind"]["type"], "stroke");
    assert_eq!(
        serde_json::from_value::<Operation>(value).unwrap(),
        add("a")
    );
    assert_eq!(
        serde_json::to_string(&ShapeKind::RightTriangle).unwrap(),
        "\"right_triangle\""
    );
}

#[test]
fn batch_add_update_delete_increments_once() {
    let mut document = Document::new();
    let mut changed = stroke("a");
    if let ObjectKind::Stroke { style, .. } = &mut changed.kind {
        style.width = 7.0;
    }
    apply(
        &mut document,
        &[
            add("a"),
            add("b"),
            Operation::Update {
                object: changed.clone(),
            },
            Operation::Delete { id: "b".into() },
        ],
    )
    .unwrap();
    assert_eq!(document.revision, 1);
    assert_eq!(document.current_page().objects, vec![changed]);
}

#[test]
fn failed_batch_is_completely_atomic() {
    let mut document = Document::new();
    let before = document.clone();
    assert!(matches!(
        apply(&mut document, &[add("a"), add("a")]),
        Err(Error::DuplicateId(_))
    ));
    assert_eq!(document, before);
    assert!(matches!(
        apply(
            &mut document,
            &[
                add("a"),
                Operation::Delete {
                    id: "missing".into()
                }
            ]
        ),
        Err(Error::ObjectNotFound(_))
    ));
    assert_eq!(document, before);
    assert!(matches!(
        apply(
            &mut document,
            &[Operation::Update {
                object: stroke("missing")
            }]
        ),
        Err(Error::ObjectNotFound(_))
    ));
    assert_eq!(document, before);
}

#[test]
fn stale_revision_and_unknown_page_are_rejected() {
    let mut document = Document::new();
    let before = document.clone();
    let page = document.current_page().id.clone();
    assert!(matches!(
        document.apply(&page, 1, &[add("a")]),
        Err(Error::RevisionConflict { .. })
    ));
    assert!(matches!(
        document.apply("unknown", 0, &[]),
        Err(Error::PageNotFound(_))
    ));
    assert_eq!(document, before);
}

#[test]
fn no_op_batches_do_not_advance_revision() {
    let mut document = Document::new();
    apply(&mut document, &[]).unwrap();
    apply(
        &mut document,
        &[add("a"), Operation::Delete { id: "a".into() }],
    )
    .unwrap();
    assert_eq!(document.revision, 0);
    apply(&mut document, &[add("a")]).unwrap();
    apply(
        &mut document,
        &[Operation::Update {
            object: stroke("a"),
        }],
    )
    .unwrap();
    assert_eq!(document.revision, 1);
}

#[test]
fn object_ids_are_unique_across_pages() {
    let mut document = Document::new();
    apply(&mut document, &[add("a")]).unwrap();
    document.add_page().unwrap();
    let before = document.clone();
    assert!(matches!(
        apply(&mut document, &[add("a")]),
        Err(Error::DuplicateId(_))
    ));
    assert_eq!(document, before);
}

#[test]
fn invalid_stroke_samples_and_widths_are_rejected_atomically() {
    let mut invalid = Vec::new();
    let mut empty = stroke("a");
    if let ObjectKind::Stroke { points, .. } = &mut empty.kind {
        points.clear();
    }
    invalid.push(empty);
    for width in [
        f32::NAN,
        f32::INFINITY,
        0.0,
        -1.0,
        MIN_BRUSH_WIDTH / 2.0,
        MAX_BRUSH_WIDTH + 1.0,
    ] {
        let mut object = stroke("a");
        if let ObjectKind::Stroke { style, .. } = &mut object.kind {
            style.width = width;
        }
        invalid.push(object);
    }
    for sample in [
        StrokePoint {
            x: f32::NAN,
            ..StrokePoint::default()
        },
        StrokePoint {
            y: f32::INFINITY,
            ..StrokePoint::default()
        },
        StrokePoint {
            time: f64::NAN,
            ..StrokePoint::default()
        },
        StrokePoint {
            time: -1.0,
            ..StrokePoint::default()
        },
        StrokePoint {
            pressure: f32::NAN,
            ..StrokePoint::default()
        },
        StrokePoint {
            pressure: 1.1,
            ..StrokePoint::default()
        },
        StrokePoint {
            pressure: -0.1,
            ..StrokePoint::default()
        },
    ] {
        let mut object = stroke("a");
        if let ObjectKind::Stroke { points, .. } = &mut object.kind {
            points[0] = sample;
        }
        invalid.push(object);
    }
    for object in invalid {
        let mut document = Document::new();
        let before = document.clone();
        assert!(apply(&mut document, &[add("valid"), Operation::Add { object }]).is_err());
        assert_eq!(document, before);
    }
    for width in [MIN_BRUSH_WIDTH, MAX_BRUSH_WIDTH] {
        let mut object = stroke("a");
        if let ObjectKind::Stroke { style, .. } = &mut object.kind {
            style.width = width;
        }
        object.validate().unwrap();
    }
}

#[test]
fn all_object_variants_round_trip() {
    let origin = Point::default();
    let kinds = vec![
        stroke("s").kind,
        ObjectKind::Shape {
            shape: ShapeKind::Cube,
            points: vec![origin, Point { x: 10.0, y: 20.0 }],
            style: Style::default(),
        },
        ObjectKind::Text {
            position: origin,
            text: "中文板书".into(),
            size: 20.0,
            color: Color::default(),
        },
        ObjectKind::Image {
            position: origin,
            width: 100.0,
            height: 50.0,
            asset_ref: "asset:1".into(),
        },
        ObjectKind::CoordinateSystem {
            origin,
            scale: 10.0,
        },
        ObjectKind::FunctionPlot {
            position: origin,
            width: 100.0,
            height: 100.0,
            expressions: vec!["sin(x)".into()],
            x_min: -10.0,
            x_max: 10.0,
            y_min: -10.0,
            y_max: 10.0,
        },
    ];
    let mut document = Document::new();
    for (index, kind) in kinds.into_iter().enumerate() {
        apply(
            &mut document,
            &[Operation::Add {
                object: BoardObject {
                    id: index.to_string(),
                    kind,
                },
            }],
        )
        .unwrap();
    }
    assert_eq!(
        Document::from_json(&document.to_json().unwrap()).unwrap(),
        document
    );
}

#[test]
fn non_stroke_validation_checks_geometry_and_ranges() {
    let bad = Point {
        x: f32::INFINITY,
        y: 0.0,
    };
    let kinds = vec![
        ObjectKind::Shape {
            shape: ShapeKind::Line,
            points: vec![Point::default(), bad],
            style: Style::default(),
        },
        ObjectKind::Shape {
            shape: ShapeKind::Circle,
            points: vec![],
            style: Style::default(),
        },
        ObjectKind::Text {
            position: bad,
            text: "a".into(),
            size: 12.0,
            color: Color::default(),
        },
        ObjectKind::Text {
            position: Point::default(),
            text: "a".into(),
            size: f32::NAN,
            color: Color::default(),
        },
        ObjectKind::Image {
            position: Point::default(),
            width: -1.0,
            height: 10.0,
            asset_ref: "asset:a".into(),
        },
        ObjectKind::CoordinateSystem {
            origin: Point::default(),
            scale: 0.0,
        },
        ObjectKind::FunctionPlot {
            position: Point::default(),
            width: 10.0,
            height: 10.0,
            expressions: vec!["x".into()],
            x_min: 1.0,
            x_max: 1.0,
            y_min: 0.0,
            y_max: 1.0,
        },
        ObjectKind::FunctionPlot {
            position: Point::default(),
            width: 10.0,
            height: 10.0,
            expressions: vec!["x".into()],
            x_min: 0.0,
            x_max: f64::INFINITY,
            y_min: 0.0,
            y_max: 1.0,
        },
    ];
    for kind in kinds {
        assert!(
            BoardObject {
                id: "a".into(),
                kind
            }
            .validate()
            .is_err()
        );
    }
    assert!(
        BoardObject {
            id: " ".into(),
            kind: stroke("a").kind
        }
        .validate()
        .is_err()
    );
}

#[test]
fn page_limit_and_last_page_are_enforced() {
    let mut document = Document::new();
    let first = document.current_page().id.clone();
    assert!(matches!(document.delete_page(&first), Err(Error::LastPage)));
    for _ in 1..MAX_PAGES {
        document.add_page().unwrap();
    }
    assert_eq!(document.pages.len(), MAX_PAGES);
    let before = document.clone();
    assert!(matches!(document.add_page(), Err(Error::PageLimit)));
    assert_eq!(document, before);
    document.pages.push(Page::new());
    assert!(matches!(document.validate(), Err(Error::PageLimit)));
}

#[test]
fn page_navigation_and_deletion_keep_valid_selection() {
    let mut document = Document::new();
    let first = document.current_page().id.clone();
    document.add_page().unwrap();
    let third = document.add_page().unwrap();
    let revision = document.revision;
    document.set_current_page(1).unwrap();
    assert_eq!(document.revision, revision);
    assert!(document.set_current_page(3).is_err());
    document.delete_page(&first).unwrap();
    assert_eq!(document.current_page, 0);
    document.set_current_page(1).unwrap();
    document.delete_page(&third).unwrap();
    assert_eq!(document.current_page, 0);
    document.validate().unwrap();
}

#[test]
fn invalid_document_structures_are_rejected() {
    let original = Document::new();
    let mut document = original.clone();
    document.pages.clear();
    assert!(document.validate().is_err());
    document = original.clone();
    document.current_page = 1;
    assert!(document.validate().is_err());
    document = original.clone();
    document.pages.push(document.pages[0].clone());
    assert!(matches!(document.validate(), Err(Error::DuplicateId(_))));
    document = original;
    document.id.clear();
    assert!(document.validate().is_err());
}

#[test]
fn history_undo_redo_monotonic_revision_and_saved_baseline() {
    let mut document = Document::new();
    let mut history = History::new(&document);
    assert!(!history.is_dirty(&document));
    assert!(!history.undo(&mut document).unwrap());
    history_apply(&mut history, &mut document, &[add("a")]).unwrap();
    assert!(history.is_dirty(&document));
    history.mark_saved(&document);
    history_apply(&mut history, &mut document, &[add("b")]).unwrap();
    assert!(history.undo(&mut document).unwrap());
    assert_eq!(document.revision, 3);
    assert!(!history.is_dirty(&document));
    assert!(history.undo(&mut document).unwrap());
    assert_eq!(document.revision, 4);
    assert!(history.is_dirty(&document));
    assert!(history.redo(&mut document).unwrap());
    assert_eq!(document.revision, 5);
    assert!(!history.is_dirty(&document));
    assert!(history.redo(&mut document).unwrap());
    assert_eq!(document.revision, 6);
    assert!(history.is_dirty(&document));
    assert!(!history.redo(&mut document).unwrap());
}

#[test]
fn no_op_and_failure_keep_redo_but_new_edit_discards_it() {
    let mut document = Document::new();
    let mut history = History::new(&document);
    history_apply(&mut history, &mut document, &[add("a")]).unwrap();
    history.undo(&mut document).unwrap();
    history_apply(&mut history, &mut document, &[]).unwrap();
    assert!(history.can_redo());
    assert!(history_apply(&mut history, &mut document, &[add("b"), add("b")]).is_err());
    assert!(history.can_redo());
    assert!(!history.can_undo());
    history_apply(&mut history, &mut document, &[add("c")]).unwrap();
    assert!(!history.can_redo());
}

#[test]
fn page_edits_are_undoable_and_navigation_is_not_dirty() {
    let mut document = Document::new();
    let mut history = History::new(&document);
    history
        .edit(&mut document, |d| {
            d.add_page()?;
            d.add_page()?;
            Ok(())
        })
        .unwrap();
    assert_eq!(document.revision, 1);
    history.mark_saved(&document);
    document.set_current_page(0).unwrap();
    assert!(!history.is_dirty(&document));
    history.undo(&mut document).unwrap();
    assert_eq!(document.pages.len(), 1);
    history.redo(&mut document).unwrap();
    assert_eq!(document.pages.len(), 3);
    assert!(!history.is_dirty(&document));
    let before = document.clone();
    assert!(
        history
            .edit(&mut document, |d| {
                d.pages.clear();
                Ok(())
            })
            .is_err()
    );
    assert_eq!(document, before);
}

#[test]
fn history_rejects_external_edits_without_discarding_them() {
    let mut document = Document::new();
    let mut history = History::new(&document);
    history_apply(&mut history, &mut document, &[add("a")]).unwrap();
    apply(&mut document, &[add("b")]).unwrap();
    let before = document.clone();
    assert!(matches!(
        history.undo(&mut document),
        Err(Error::HistoryDiverged)
    ));
    assert_eq!(document, before);
}

#[test]
fn revision_overflow_never_partially_changes_state() {
    let mut document = Document::new();
    document.revision = u64::MAX;
    let before = document.clone();
    assert!(matches!(
        apply(&mut document, &[add("a")]),
        Err(Error::RevisionOverflow)
    ));
    assert_eq!(document, before);
    assert!(matches!(document.add_page(), Err(Error::RevisionOverflow)));
    assert_eq!(document, before);
    apply(&mut document, &[]).unwrap();
    document.revision = u64::MAX - 1;
    let mut history = History::new(&document);
    history_apply(&mut history, &mut document, &[add("a")]).unwrap();
    let before = document.clone();
    assert!(matches!(
        history.undo(&mut document),
        Err(Error::RevisionOverflow)
    ));
    assert!(history.can_undo());
    assert!(!history.can_redo());
    assert_eq!(document, before);
}

#[test]
fn json_rejects_future_versions_truncation_and_invalid_documents() {
    assert!(matches!(
        from_json(r#"{"version":999,"document":{}}"#),
        Err(Error::UnsupportedVersion(999))
    ));
    assert!(from_json("{}").is_err());
    assert!(from_json("{\"version\":1,").is_err());
    let mut value: serde_json::Value =
        serde_json::from_str(&Document::new().to_json().unwrap()).unwrap();
    value["document"]["pages"] = serde_json::json!([]);
    assert!(from_json(&value.to_string()).is_err());
}

struct TestDirectory(std::path::PathBuf);
impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("board-core-test-{}", new_id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn atomic_save_replaces_existing_file_and_cleans_temporary_files() {
    let dir = TestDirectory::new();
    let path = dir.0.join("板书.json");
    let mut document = Document::new();
    document.save(&path).unwrap();
    apply(&mut document, &[add("a")]).unwrap();
    document.save(&path).unwrap();
    assert_eq!(Document::load(&path).unwrap(), document);
    assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    let saved = fs::read(&path).unwrap();
    document.current_page = 100;
    assert!(document.save(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), saved);
    assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
}

#[test]
fn failed_save_preserves_dirty_and_existing_target() {
    let dir = TestDirectory::new();
    let mut document = Document::new();
    let mut history = History::new(&document);
    history_apply(&mut history, &mut document, &[add("a")]).unwrap();
    let destination = dir.0.join("directory");
    fs::create_dir(&destination).unwrap();
    assert!(history.save(&document, &destination).is_err());
    assert!(destination.is_dir());
    assert!(history.is_dirty(&document));
    assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    history.save(&document, dir.0.join("saved.json")).unwrap();
    assert!(!history.is_dirty(&document));
}
