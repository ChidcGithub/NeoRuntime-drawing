use super::*;

fn sample(y: f32) -> Vec<HandwritingStroke> {
    vec![HandwritingStroke {
        points: vec![
            StrokePoint {
                x: 20.0,
                y,
                time: 10.0,
                pressure: 0.25,
            },
            StrokePoint {
                x: 56.0,
                y: y + 4.0,
                time: 10.5,
                pressure: 0.75,
            },
        ],
        style: Style {
            width: 4.0,
            dashed: true,
            ..Style::default()
        },
    }]
}

fn text_kind(text: &str, size: f32) -> ObjectKind {
    ObjectKind::Text {
        position: Point { x: 100.0, y: 200.0 },
        text: text.into(),
        size,
        color: Color {
            r: 20,
            g: 40,
            b: 60,
            a: 180,
        },
    }
}

fn strokes(kind: &ObjectKind) -> &[HandwritingStroke] {
    match kind {
        ObjectKind::Handwritten { strokes, .. } => strokes,
        _ => panic!("Expected handwriting"),
    }
}

fn assert_core_valid(kind: ObjectKind) {
    board_core::BoardObject {
        id: "handwriting-test".into(),
        kind,
    }
    .validate()
    .unwrap();
}

#[test]
fn deterministic_variants_cycle_across_occurrences_without_mutation() {
    let mut profile = Profile::default();
    for y in [24.0, 48.0, 72.0] {
        profile.add_sample('x', sample(y)).unwrap();
    }
    let before = profile.clone();
    let kind = text_kind("xxxx", 72.0);
    let rendered = profile.render(&kind, "xxxx").unwrap();
    assert_eq!(rendered, profile.render(&kind, "xxxx").unwrap());
    assert_eq!(profile, before);
    let ink = strokes(&rendered);
    assert_eq!(
        ink.iter().map(|s| s.points[0].y).collect::<Vec<_>>(),
        [24.0, 48.0, 72.0, 24.0]
    );
    let radius = ink_extent(4.0, 1.0);
    for (i, stroke) in ink.iter().enumerate() {
        let expected = radius + i as f32 * (36.0 + 2.0 * radius + 12.0);
        assert!((stroke.points[0].x - expected).abs() < 0.0001);
    }
    assert!(
        profile
            .add_sample('x', sample(30.0))
            .unwrap_err()
            .contains("delete")
    );
    assert_eq!(profile, before);
    assert_core_valid(rendered);
}

#[test]
fn thick_vertical_digits_reserve_both_pen_extents_at_clamped_scales() {
    let mut profile = Profile::default();
    let mut digit = sample(24.0);
    digit[0].points[1].x = digit[0].points[0].x;
    digit[0].points[1].y = 96.0;
    digit[0].style.width = 100.0;
    profile.add_sample('1', digit).unwrap();
    for size in [0.01, 26.0, 72.0, 144.0] {
        let scale = size / CAP_HEIGHT;
        let result = profile.render(&text_kind("11", size), "11").unwrap();
        let ink = strokes(&result);
        let radius = ink_extent(ink[0].style.width, 1.0);
        let right = ink[0].points[0].x + radius;
        let left = ink[1].points[0].x - radius;
        assert!(left > right, "Digits overlap at size {size}");
        assert!((left - right - 12.0 * scale).abs() < 0.0001);
        assert!((ink[0].points[0].x - radius).abs() < 0.0001);
        assert_core_valid(result);
    }
}

#[test]
fn vertical_pen_overhang_pads_lines_and_math_without_changing_baseline_offsets() {
    let mut profile = Profile::default();
    let mut digit = sample(0.0);
    digit[0].points[1].x = digit[0].points[0].x;
    digit[0].points[1].y = 128.0;
    digit[0].style.width = 100.0;
    profile.add_sample('1', digit).unwrap();
    profile.add_sample('.', sample(94.0)).unwrap();
    let mut planner = Planner {
        profile: &profile,
        occurrences: BTreeMap::new(),
        strokes: 0,
        points: 0,
        pen_width: profile.median_width(),
    };
    let plan = planner.text("1.\n1", 72.0).unwrap();
    let radius = ink_extent(100.0, 1.0);
    assert_eq!(plan.baseline, 96.0 + radius);
    let result = profile.render(&text_kind("1.\n1", 72.0), "1.\n1").unwrap();
    let ink = strokes(&result);
    assert_eq!(ink[0].points[0].y - plan.baseline, -96.0);
    assert_eq!(ink[1].points[0].y - plan.baseline, -2.0);
    assert!((ink[2].points[0].y - radius - (ink[0].points[1].y + radius) - 16.0).abs() < 0.0001);
    assert_core_valid(result);

    let kind = ObjectKind::Math {
        position: Point::default(),
        size: 72.0,
        color: Color::default(),
        layout: MathLayout::Fraction(
            Box::new(MathLayout::Text("1".into())),
            Box::new(MathLayout::Text("1".into())),
        ),
    };
    let result = profile.render(&kind, "1/1").unwrap();
    let ink = strokes(&result);
    let bar_top = ink[2].points[0].y - ink[2].style.width / 2.0;
    let bar_bottom = ink[2].points[0].y + ink[2].style.width / 2.0;
    assert!(ink[0].points[1].y + ink[0].style.width / 2.0 < bar_top);
    assert!(ink[1].points[0].y - ink[1].style.width / 2.0 > bar_bottom);
    assert_core_valid(result);
}

// Exercise the production renderer without a native window. Disable AA feather
// so these bounds measure ink geometry, not the display-only transparent fringe.
fn mesh_bounds(stroke: &HandwritingStroke) -> egui::Rect {
    let ctx = egui::Context::default();
    ctx.tessellation_options_mut(|options| options.feathering = false);
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        board_render::paint_object(
            &ui.ctx()
                .layer_painter(egui::LayerId::background())
                .with_clip_rect(egui::Rect::from_min_max(
                    egui::pos2(-1000.0, -1000.0),
                    egui::pos2(10000.0, 10000.0),
                )),
            &board_core::BoardObject {
                id: "geometry-probe".into(),
                kind: ObjectKind::Handwritten {
                    position: Point::default(),
                    text: "x".into(),
                    layout: None,
                    strokes: vec![stroke.clone()],
                },
            },
        );
    });
    output.textures_delta.clear();
    let mut bounds = egui::Rect::NOTHING;
    for shape in output.shapes {
        if let egui::Shape::Mesh(mesh) = shape.shape {
            for vertex in &mesh.vertices {
                bounds.extend_with(vertex.pos);
            }
        }
    }
    assert!(bounds.is_finite(), "Expected actual renderer mesh");
    bounds
}

fn sharp_sample(transpose: bool) -> Vec<HandwritingStroke> {
    vec![HandwritingStroke {
        points: [(28.0, 52.0), (100.0, 64.0), (28.0, 76.0)]
            .into_iter()
            .enumerate()
            .map(|(i, (x, y))| StrokePoint {
                x: if transpose { y } else { x },
                y: if transpose { x } else { y },
                // Slow motion keeps the join close to the full pen width.
                time: i as f64 * 100.0,
                pressure: 1.0,
            })
            .collect(),
        style: Style {
            width: 100.0,
            ..Style::default()
        },
    }]
}

#[test]
fn sharp_join_actual_mesh_gaps_cover_x_y_multiline_and_fraction_at_clamped_widths() {
    for transpose in [false, true] {
        let mut profile = Profile::default();
        profile.add_sample('>', sharp_sample(transpose)).unwrap();
        let mut vertical = fallback_stem();
        vertical[0].style.width = 100.0;
        vertical[0].points[1].time = 100.0;
        profile.add_sample('1', vertical).unwrap();
        for size in [0.01, 26.0, 72.0, 144.0] {
            let scale = size / CAP_HEIGHT;
            for text in [">1", ">\n1"] {
                let rendered = profile.render(&text_kind(text, size), text).unwrap();
                let a = mesh_bounds(&strokes(&rendered)[0]);
                let b = mesh_bounds(&strokes(&rendered)[1]);
                if text.contains('\n') {
                    assert!(b.min.y - a.max.y >= 16.0 * scale - 0.0001);
                } else {
                    assert!(b.min.x - a.max.x >= 12.0 * scale - 0.0001);
                }
                assert!(a.min.x >= -0.0001 && a.min.y >= -0.0001);
                assert_core_valid(rendered);
            }
            let layout = MathLayout::Fraction(
                Box::new(MathLayout::Text(">".into())),
                Box::new(MathLayout::Text("1".into())),
            );
            let kind = ObjectKind::Math {
                position: Point::default(),
                size,
                color: Color::default(),
                layout,
            };
            let rendered = profile.render(&kind, ">/1").unwrap();
            let ink = strokes(&rendered);
            let numerator = mesh_bounds(&ink[0]);
            let denominator = mesh_bounds(&ink[1]);
            let bar = mesh_bounds(&ink[2]);
            assert!(bar.min.y - numerator.max.y >= size * 0.15 - 0.0001);
            assert!(denominator.min.y - bar.max.y >= size * 0.15 - 0.0001);
            assert_core_valid(rendered);
        }
    }
    let sharp = sharp_sample(false);
    let bounds = mesh_bounds(&sharp[0]);
    assert!(
        bounds.max.x > 150.0,
        "Fixture must exceed the old half-width bound"
    );
}

#[test]
fn fraction_axis_matches_authored_operators_in_rows_and_nested_layouts() {
    let profile = Profile::default();
    for size in [0.01, 26.0, 72.0, 144.0] {
        let fraction = MathLayout::Fraction(
            Box::new(MathLayout::Text("1".into())),
            Box::new(MathLayout::Text("2".into())),
        );
        let row = MathLayout::Row(vec![fraction, MathLayout::Text("-−+=×÷±≈".into())]);
        for layout in [
            row.clone(),
            MathLayout::Fraction(Box::new(row.clone()), Box::new(row.clone())),
        ] {
            let kind = ObjectKind::Math {
                position: Point::default(),
                size,
                color: Color::default(),
                layout,
            };
            let rendered = profile
                .render_adaptive(&kind, "axis", |ch| {
                    crate::handwriting_fallback::sample(ch, None)
                })
                .unwrap();
            let ink = strokes(&rendered);
            // Each row: two digits, bar, then operators with 1/1/2/2/2/3/3/2 strokes.
            for row_start in [0, 19].into_iter().take(if ink.len() > 20 { 2 } else { 1 }) {
                let axis = ink[row_start + 2].points[0].y;
                let mut offset = row_start + 3;
                for count in [1, 1, 2, 2, 2, 3, 3, 2] {
                    let points = ink[offset..offset + count].iter().flat_map(|s| &s.points);
                    let (mut top, mut bottom) = (f32::INFINITY, f32::NEG_INFINITY);
                    for p in points {
                        top = top.min(p.y);
                        bottom = bottom.max(p.y);
                    }
                    assert!(
                        ((top + bottom) * 0.5 - axis).abs() < 0.0001,
                        "operator at {offset}, size {size}"
                    );
                    offset += count;
                }
            }
            assert_core_valid(rendered);
        }
    }
}

#[test]
fn missing_characters_return_whole_result_error_not_fake_ink() {
    let mut profile = Profile::default();
    profile.add_sample('x', sample(24.0)).unwrap();
    let before = profile.clone();
    let error = profile
        .render(&text_kind("x中b中a", 26.0), "x中b中a")
        .unwrap_err();
    assert_eq!(error, "Missing handwriting samples: ab中");
    assert_eq!(profile, before);
    assert!(profile.render(&text_kind(" ", 26.0), " ").is_err());
    assert!(profile.render(&text_kind("x", 26.0), "other").is_err());
    assert!(
        profile
            .render(
                &ObjectKind::CoordinateSystem {
                    origin: Point::default(),
                    scale: 1.0
                },
                "x"
            )
            .is_err()
    );
}

#[test]
fn metadata_scaling_preserves_relative_time_pressure_color_and_baseline() {
    let mut profile = Profile::default();
    profile.add_sample('.', sample(94.0)).unwrap();
    profile.add_sample('-', sample(60.0)).unwrap();
    let original = text_kind(".-", 36.0);
    let rendered = profile.render(&original, ".-").unwrap();
    let ink = strokes(&rendered);
    assert_eq!(ink[0].points[0].y, 47.0);
    assert_eq!(ink[1].points[0].y, 30.0);
    assert_eq!(ink[0].points[0].time, 0.0);
    assert_eq!(ink[0].points[1].time, 0.25);
    assert_eq!(ink[0].points[0].pressure, 0.25);
    assert_eq!(ink[0].points[1].pressure, 0.75);
    assert_eq!(ink[0].style.width, 2.0);
    assert!(ink[0].style.dashed);
    if let ObjectKind::Text {
        color, position, ..
    } = original
    {
        assert_eq!(ink[0].style.color, color);
        assert!(
            matches!(&rendered, ObjectKind::Handwritten { position: p, text, layout: None, .. } if *p == position && text == ".-")
        );
    }
    let speed = |s: &HandwritingStroke| {
        let a = s.points[0];
        let b = s.points[1];
        f64::from(b.x - a.x).hypot(f64::from(b.y - a.y)) / (b.time - a.time)
    };
    assert_eq!(speed(&ink[0]), speed(&sample(94.0)[0]));
    assert_core_valid(rendered);
    for size in [0.01, 7200.0] {
        let result = profile.render(&text_kind(".", size), ".").unwrap();
        assert!((MIN_BRUSH_WIDTH..=MAX_BRUSH_WIDTH).contains(&strokes(&result)[0].style.width));
    }
}

#[test]
fn whitespace_multiline_and_crlf_need_no_samples() {
    let mut profile = Profile::default();
    profile.add_sample('x', sample(24.0)).unwrap();
    let source = "x \tx\r\nx\rx\u{2028}x";
    let result = profile.render(&text_kind(source, 72.0), source).unwrap();
    let ink = strokes(&result);
    assert_eq!(ink.len(), 5);
    let radius = ink_extent(4.0, 1.0);
    assert!((ink[1].points[0].x - (48.0 + 2.0 * radius + 36.0 + 144.0 + radius)).abs() < 0.0001);
    assert_eq!(
        ink.iter().map(|s| s.points[0].y).collect::<Vec<_>>(),
        [24.0, 24.0, 168.0, 312.0, 456.0]
    );
    assert_core_valid(result);
}

#[test]
fn recursive_fraction_radical_retains_semantics_and_uses_median_structure_width() {
    let mut profile = Profile::default();
    for (label, width) in [('1', 2.0), ('2', 6.0)] {
        let mut ink = sample(24.0);
        ink[0].style.width = width;
        profile.add_sample(label, ink).unwrap();
    }
    let layout = MathLayout::Row(vec![
        MathLayout::Text("1".into()),
        MathLayout::Radical(Box::new(MathLayout::Fraction(
            Box::new(MathLayout::Text("1".into())),
            Box::new(MathLayout::Text("2".into())),
        ))),
    ]);
    let original = "1 + √(1/2) (exact supplied semantics)";
    let kind = ObjectKind::Math {
        position: Point::default(),
        layout: layout.clone(),
        size: 72.0,
        color: Color::default(),
    };
    let result = profile.render(&kind, original).unwrap();
    assert!(
        matches!(&result, ObjectKind::Handwritten { text, layout: Some(l), .. } if text == original && *l == layout)
    );
    let ink = strokes(&result);
    assert_eq!(ink.len(), 5);
    let bar = &ink[3];
    let radical = &ink[4];
    assert_eq!(bar.points.len(), 2);
    assert_eq!(radical.points.len(), 5);
    assert_eq!(bar.points[0].y, bar.points[1].y);
    assert_eq!(bar.style.width, 4.0);
    assert_eq!(radical.style.width, 4.0);
    assert!(!bar.style.dashed);
    assert!(ink[1].points[1].y < bar.points[0].y);
    assert!(ink[2].points[0].y > bar.points[0].y);
    assert_eq!(radical.points[3].y, radical.points[4].y);
    assert!(radical.points[3].y < ink[1].points[0].y);
    assert_eq!(result, profile.render(&kind, original).unwrap());
    assert_core_valid(result);
}

#[test]
fn variants_are_shared_across_math_leaves() {
    let mut profile = Profile::default();
    profile.add_sample('x', sample(24.0)).unwrap();
    profile.add_sample('x', sample(48.0)).unwrap();
    let kind = ObjectKind::Math {
        position: Point::default(),
        size: 72.0,
        color: Color::default(),
        layout: MathLayout::Row(vec![
            MathLayout::Text("x".into()),
            MathLayout::Text("xx".into()),
        ]),
    };
    let result = profile.render(&kind, "xxx").unwrap();
    assert_eq!(
        strokes(&result)
            .iter()
            .map(|s| s.points[0].y)
            .collect::<Vec<_>>(),
        [24.0, 48.0, 24.0]
    );
}

#[test]
fn sample_validation_is_transactional() {
    let mut profile = Profile::default();
    profile.add_sample('中', sample(24.0)).unwrap();
    let before = profile.clone();
    for label in [' ', '\n', '\0'] {
        assert!(profile.add_sample(label, sample(24.0)).is_err());
    }
    let mut invalid = vec![
        vec![],
        vec![HandwritingStroke {
            points: vec![],
            style: Style::default(),
        }],
        vec![sample(24.0)[0].clone(); 17],
    ];
    let mut too_many = sample(24.0);
    let point = too_many[0].points[0];
    too_many[0].points.resize(2049, point);
    invalid.push(too_many);
    for bad in [f32::NAN, f32::INFINITY, -1.0, 129.0] {
        let mut ink = sample(24.0);
        ink[0].points[0].x = bad;
        invalid.push(ink);
        let mut ink = sample(24.0);
        ink[0].points[0].y = bad;
        invalid.push(ink);
    }
    for bad in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        let mut ink = sample(24.0);
        ink[0].points[0].pressure = bad;
        invalid.push(ink);
    }
    for bad in [f32::NAN, f32::INFINITY, 0.0, 101.0] {
        let mut ink = sample(24.0);
        ink[0].style.width = bad;
        invalid.push(ink);
    }
    for bad in [
        f64::NAN,
        f64::INFINITY,
        -1.0,
        MAX_HANDWRITING_TIME + 1.0,
        9.0,
    ] {
        let mut ink = sample(24.0);
        ink[0].points[1].time = bad;
        invalid.push(ink);
    }
    for ink in invalid {
        assert!(profile.add_sample('x', ink).is_err());
        assert_eq!(profile, before);
    }
}

#[test]
fn profile_counts_removal_and_glyph_limit() {
    let mut profile = Profile::default();
    for value in 0x4e00..0x4e00 + MAX_GLYPHS as u32 {
        profile
            .add_sample(char::from_u32(value).unwrap(), sample(24.0))
            .unwrap();
    }
    assert_eq!(profile.counts(), (96, 96));
    assert_eq!(profile.labels().chars().count(), 96);
    assert!(profile.add_sample('x', sample(24.0)).is_err());
    profile.add_sample('一', sample(48.0)).unwrap();
    assert_eq!(profile.sample_count('一'), 2);
    profile.remove('一');
    assert_eq!(profile.sample_count('一'), 0);
    profile.add_sample('x', sample(24.0)).unwrap();
    profile.clear();
    assert_eq!(profile, Profile::default());
}

#[test]
fn total_profile_and_output_point_budgets_are_enforced() {
    let mut profile = Profile::default();
    let mut ink = sample(24.0);
    let point = ink[0].points[1];
    ink[0].points.resize(MAX_SAMPLE_POINTS, point);
    for label in 'A'..='`' {
        profile.add_sample(label, ink.clone()).unwrap();
    }
    assert_eq!(profile.validate().unwrap(), MAX_PROFILE_POINTS);
    let before = profile.clone();
    assert!(profile.add_sample('a', sample(24.0)).is_err());
    assert_eq!(before, profile);
    let valid = "A".repeat(MAX_HANDWRITING_POINTS / MAX_SAMPLE_POINTS);
    assert_core_valid(profile.render(&text_kind(&valid, 26.0), &valid).unwrap());
    let invalid = format!("{valid}A");
    assert!(
        profile
            .render(&text_kind(&invalid, 26.0), &invalid)
            .unwrap_err()
            .contains("budget")
    );
    let mut invalid_profile = profile.clone();
    invalid_profile.glyphs[0].variants.push(sample(24.0));
    let json = serde_json::to_string(&ProfileFile {
        version: VERSION,
        profile: invalid_profile,
    })
    .unwrap();
    assert!(Profile::from_json(&json).is_err());
}

#[test]
fn output_stroke_geometry_time_and_input_budgets_are_enforced() {
    let mut profile = Profile::default();
    let ink = vec![
        HandwritingStroke {
            points: vec![sample(24.0)[0].points[0]],
            style: Style::default()
        };
        16
    ];
    profile.add_sample('x', ink).unwrap();
    let good = "x".repeat(64);
    assert_core_valid(profile.render(&text_kind(&good, 26.0), &good).unwrap());
    let bad = "x".repeat(65);
    assert!(profile.render(&text_kind(&bad, 26.0), &bad).is_err());
    for size in [f32::NAN, f32::INFINITY, 0.0, -1.0, 1_000_000.0] {
        assert!(profile.render(&text_kind("x", size), "x").is_err());
    }
    let mut edge = text_kind("x", 26.0);
    if let ObjectKind::Text { position, .. } = &mut edge {
        position.y = MAX_HANDWRITING_COORD;
    }
    assert!(profile.render(&edge, "x").is_err());
    let text = "x".repeat(MAX_INPUT_CHARS + 1);
    assert!(profile.render(&text_kind(&text, 26.0), &text).is_err());
    assert!(profile.render(&text_kind("x\0", 26.0), "x\0").is_err());
    let mut slow = sample(24.0);
    slow[0].points[0].time = 0.0;
    slow[0].points[1].time = MAX_HANDWRITING_TIME;
    profile.add_sample('s', slow).unwrap();
    assert!(profile.render(&text_kind("s", 144.0), "s").is_err());
}

#[test]
fn layout_depth_node_and_leaf_budgets_are_checked_before_recursion() {
    let profile = Profile::default();
    let mut deep = MathLayout::Text(" ".into());
    for _ in 0..board_core::MAX_MATH_DEPTH {
        deep = MathLayout::Radical(Box::new(deep));
    }
    let layouts = [
        deep,
        MathLayout::Row(vec![
            MathLayout::Text(String::new());
            board_core::MAX_MATH_NODES
        ]),
        MathLayout::Text(" ".repeat(board_core::MAX_MATH_TEXT_BYTES + 1)),
    ];
    for layout in layouts {
        let kind = ObjectKind::Math {
            position: Point::default(),
            layout,
            size: 26.0,
            color: Color::default(),
        };
        assert!(profile.render(&kind, "original").is_err());
    }
}

#[test]
fn json_roundtrip_version_validation_and_import_budgets() {
    let mut profile = Profile::default();
    profile.add_sample('中', sample(24.0)).unwrap();
    profile.add_sample('x', sample(48.0)).unwrap();
    let json = profile.to_json().unwrap();
    assert_eq!(Profile::from_json(&json).unwrap(), profile);
    assert_eq!(Profile::from_json(&json).unwrap().to_json().unwrap(), json);
    for bad in [
        "{}",
        "null",
        "{",
        "{\"version\":1,\"profile\":{\"glyphs\":[]},\"extra\":0}",
    ] {
        assert!(Profile::from_json(bad).is_err());
    }
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    for version in [0, 3, 100] {
        let mut bad = value.clone();
        bad["version"] = version.into();
        assert!(
            Profile::from_json(&bad.to_string())
                .unwrap_err()
                .contains("version")
        );
    }
    let mut duplicate = value.clone();
    let glyph = duplicate["profile"]["glyphs"][0].clone();
    duplicate["profile"]["glyphs"]
        .as_array_mut()
        .unwrap()
        .push(glyph);
    assert!(Profile::from_json(&duplicate.to_string()).is_err());
    let mut bad = value.clone();
    bad["profile"]["glyphs"][0]["variants"][0][0]["points"][0]["x"] = 129.into();
    assert!(Profile::from_json(&bad.to_string()).is_err());
    let mut bad = value;
    bad["profile"]["glyphs"][0]["variants"] = serde_json::json!([]);
    assert!(Profile::from_json(&bad.to_string()).is_err());
    assert!(Profile::from_json(&" ".repeat(MAX_JSON_BYTES + 1)).is_err());
    assert_eq!(
        Profile::from_json(&profile.to_json().unwrap()).unwrap(),
        profile
    );
    let invalid: Profile =
        serde_json::from_str("{\"glyphs\":[{\"label\":\"x\",\"variants\":[]}]}").unwrap();
    assert!(invalid.to_json().is_err());
    assert!(invalid.render(&text_kind("x", 26.0), "x").is_err());
}

fn learning_stroke(segments: usize) -> Vec<StrokePoint> {
    (0..=segments)
        .map(|i| {
            let t = i as f32 / segments as f32;
            StrokePoint {
                x: 300.0 + 36.0 * t,
                y: 200.0 + 72.0 * t,
                time: 10.0 + f64::from(t) * 0.8,
                pressure: 1.0,
            }
        })
        .collect()
}

fn fallback_stem() -> Vec<HandwritingStroke> {
    vec![HandwritingStroke {
        points: vec![
            StrokePoint {
                x: 40.0,
                y: 24.0,
                time: 0.0,
                pressure: 1.0,
            },
            StrokePoint {
                x: 40.0,
                y: 96.0,
                time: 0.3,
                pressure: 1.0,
            },
        ],
        style: Style::default(),
    }]
}

#[test]
fn observed_style_changes_synthetic_slant_width_aspect_and_speed_only() {
    let mut profile = Profile::default();
    profile.add_sample('m', sample(24.0)).unwrap();
    let manual = profile.render(&text_kind("m", 72.0), "m").unwrap();
    let kind = text_kind("?", 72.0);
    let default_ink = profile
        .render_adaptive(&kind, "?", |_| Ok(fallback_stem()))
        .unwrap();
    assert!(profile.style_summary().contains("not learned"));
    for _ in 0..40 {
        assert!(profile.observe_stroke(
            &learning_stroke(8),
            Style {
                width: 8.0,
                ..Style::default()
            }
        ));
    }
    assert_eq!(profile.learned_strokes(), 40);
    assert!(profile.style_summary().contains("Locally learned"));
    let before = profile.clone();
    let learned = profile
        .render_adaptive(&kind, "?", |_| Ok(fallback_stem()))
        .unwrap();
    assert_eq!(
        learned,
        profile
            .render_adaptive(&kind, "?", |_| Ok(fallback_stem()))
            .unwrap()
    );
    assert_eq!(profile, before);
    assert_eq!(profile.sample_count('?'), 0);
    let a = &strokes(&default_ink)[0];
    let b = &strokes(&learned)[0];
    assert!(b.style.width > a.style.width + 3.0);
    assert!(b.points[1].x - b.points[0].x > 20.0);
    assert!(a.points[1].x < a.points[0].x);
    assert!(b.points[1].time > a.points[1].time);
    assert_eq!(b.points[0].y, 24.0);
    assert_eq!(b.points[1].y, 96.0);
    let wide_default = Profile::default()
        .render_adaptive(&kind, "?", |_| Ok(sample(48.0)))
        .unwrap();
    let wide_learned = profile
        .render_adaptive(&kind, "?", |_| Ok(sample(48.0)))
        .unwrap();
    let width = |kind: &ObjectKind| strokes(kind)[0].points[1].x - strokes(kind)[0].points[0].x;
    assert!(width(&wide_learned) < width(&wide_default));
    assert_eq!(manual, profile.render(&text_kind("m", 72.0), "m").unwrap());
    assert_eq!(
        manual,
        profile
            .render_adaptive(&text_kind("m", 72.0), "m", |_| panic!("manual must win"))
            .unwrap()
    );
    let mixed = profile
        .render_adaptive(&text_kind("m?", 72.0), "m?", |_| Ok(fallback_stem()))
        .unwrap();
    assert_eq!(strokes(&mixed)[0], strokes(&manual)[0]);
    assert_core_valid(mixed);
    assert_core_valid(learned);
    profile.clear();
    assert_eq!(profile, Profile::default());
}

#[test]
fn invalid_diagram_degenerate_and_tiny_strokes_do_not_update_style() {
    let good = learning_stroke(8);
    let mut profile = Profile::default();
    assert!(profile.observe_stroke(&good, Style::default()));
    let before = profile.clone();
    let mut cases = vec![
        vec![],
        vec![good[0]],
        vec![good[0]; 8],
        vec![good[0]; MAX_SAMPLE_POINTS + 1],
    ];
    for field in 0..9 {
        let mut bad = good.clone();
        match field {
            0 => bad[1].x = f32::NAN,
            1 => bad[1].y = f32::INFINITY,
            2 => bad[1].pressure = -0.1,
            3 => bad[1].time = 9.0,
            4 => bad[1].time = f64::NAN,
            5 => bad.iter_mut().for_each(|p| {
                p.x *= 10.0;
                p.y *= 10.0;
            }),
            6 => bad.iter_mut().for_each(|p| {
                p.x *= 0.01;
                p.y *= 0.01;
            }),
            7 => bad.iter_mut().for_each(|p| p.y = 100.0),
            _ => bad.last_mut().unwrap().time = 100.0,
        }
        cases.push(bad);
    }
    for bad in cases {
        assert!(!profile.observe_stroke(&bad, Style::default()));
        assert_eq!(profile, before);
    }
    for style in [
        Style {
            width: f32::NAN,
            ..Style::default()
        },
        Style {
            width: 100.0,
            ..Style::default()
        },
        Style {
            dashed: true,
            ..Style::default()
        },
        Style {
            color: Color {
                a: 0,
                ..Color::default()
            },
            ..Style::default()
        },
    ] {
        assert!(!profile.observe_stroke(&good, style));
        assert_eq!(profile, before);
    }
}

#[test]
fn numeric_learning_is_density_independent_bounded_and_robust_to_outliers() {
    let mut sparse = Profile::default();
    let mut dense = Profile::default();
    for _ in 0..1000 {
        assert!(sparse.observe_stroke(&learning_stroke(1), Style::default()));
        assert!(dense.observe_stroke(&learning_stroke(128), Style::default()));
    }
    let metrics = |profile: &Profile| {
        serde_json::from_str::<serde_json::Value>(&profile.to_json().unwrap()).unwrap()["profile"]["style"].clone()
    };
    let a = metrics(&sparse);
    let b = metrics(&dense);
    for key in ["pen_width", "slant", "aspect", "speed"] {
        assert!(
            (a[key].as_f64().unwrap() - b[key].as_f64().unwrap()).abs() < 0.001,
            "{key}"
        );
    }
    assert_eq!(a["recent"].as_array().unwrap().len(), 31);
    assert!(sparse.to_json().unwrap().len() < 6000);
    let before = sparse.clone();
    assert!(sparse.observe_stroke(
        &learning_stroke(1),
        Style {
            width: 24.0,
            ..Style::default()
        }
    ));
    assert!(
        (metrics(&sparse)["pen_width"].as_f64().unwrap() - a["pen_width"].as_f64().unwrap()).abs()
            < 0.001
    );
    assert_eq!(sparse.counts(), (0, 0));
    let mut serialized: serde_json::Value =
        serde_json::from_str(&before.to_json().unwrap()).unwrap();
    serialized["profile"]["style"]["learned_strokes"] = u32::MAX.into();
    let mut saturated = Profile::from_json(&serialized.to_string()).unwrap();
    assert!(saturated.observe_stroke(&learning_stroke(1), Style::default()));
    assert_eq!(saturated.learned_strokes(), u32::MAX);
    assert!(saturated.to_json().is_ok());
}

#[test]
fn learning_normalizes_size_and_caps_each_metric_update() {
    let mut normal = Profile::default();
    let mut scaled = Profile::default();
    let points = learning_stroke(8);
    let mut larger = points.clone();
    for point in &mut larger {
        point.x *= 2.0;
        point.y *= 2.0;
    }
    assert!(normal.observe_stroke(
        &points,
        Style {
            width: 8.0,
            ..Style::default()
        }
    ));
    assert!(scaled.observe_stroke(
        &larger,
        Style {
            width: 16.0,
            ..Style::default()
        }
    ));
    assert_eq!(normal, scaled);
    let value: serde_json::Value = serde_json::from_str(&normal.to_json().unwrap()).unwrap();
    let style = &value["profile"]["style"];
    assert!((style["pen_width"].as_f64().unwrap() - 3.0).abs() <= 0.60001);
    assert!((style["slant"].as_f64().unwrap() + 0.08).abs() <= 0.04001);
    assert!((style["aspect"].as_f64().unwrap() - 0.7).abs() <= 0.08001);
    assert!((style["speed"].as_f64().unwrap() - 240.0).abs() <= 40.00001);
    assert!(style["recent"][0].get("points").is_none());
}

#[test]
fn adaptive_rejects_invalid_fallback_and_input_without_learning() {
    let profile = Profile::default();
    for sample in [
        vec![],
        vec![fallback_stem()[0].clone(); MAX_SAMPLE_STROKES + 1],
        vec![HandwritingStroke {
            points: vec![fallback_stem()[0].points[0]; MAX_SAMPLE_POINTS + 1],
            style: Style::default(),
        }],
        vec![HandwritingStroke {
            points: vec![StrokePoint {
                x: 129.0,
                y: 96.0,
                time: 0.0,
                pressure: 1.0,
            }],
            style: Style::default(),
        }],
    ] {
        assert!(
            profile
                .render_adaptive(&text_kind("?", 26.0), "?", |_| Ok(sample.clone()))
                .is_err()
        );
    }
    assert!(
        profile
            .render_adaptive(&text_kind("?", f32::NAN), "?", |_| panic!("invalid size"))
            .is_err()
    );
    assert!(
        profile
            .render_adaptive(&text_kind("?", 26.0), "x", |_| panic!("semantic mismatch"))
            .is_err()
    );
    let mut large = fallback_stem();
    large[0].points = (0..MAX_SAMPLE_POINTS)
        .map(|i| StrokePoint {
            x: if i % 2 == 0 { 24.0 } else { 96.0 },
            y: 96.0,
            time: 0.0,
            pressure: 1.0,
        })
        .collect();
    let text = "?".repeat(MAX_HANDWRITING_POINTS / MAX_SAMPLE_POINTS + 1);
    assert!(
        profile
            .render_adaptive(&text_kind(&text, 26.0), &text, |_| Ok(large.clone()))
            .unwrap_err()
            .contains("budget")
    );
    assert_eq!(profile, Profile::default());
}

#[test]
fn profile_v1_migrates_v2_optional_style_roundtrips_and_validates() {
    let mut profile = Profile::default();
    profile.add_sample('x', sample(24.0)).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&profile.to_json().unwrap()).unwrap();
    assert_eq!(value["version"], 2);
    assert!(value["profile"].get("style").is_none());
    value["version"] = 1.into();
    assert_eq!(Profile::from_json(&value.to_string()).unwrap(), profile);
    assert_eq!(profile.learned_strokes(), 0);
    assert!(profile.observe_stroke(&learning_stroke(8), Style::default()));
    assert_eq!(
        Profile::from_json(&profile.to_json().unwrap()).unwrap(),
        profile
    );
    let value: serde_json::Value = serde_json::from_str(&profile.to_json().unwrap()).unwrap();
    for (field, invalid) in [
        ("pen_width", serde_json::json!(0)),
        ("slant", serde_json::json!(0.61)),
        ("aspect", serde_json::json!(2)),
        ("speed", serde_json::json!(901)),
        ("learned_strokes", serde_json::json!(0)),
        ("recent", serde_json::json!([])),
    ] {
        let mut bad = value.clone();
        bad["profile"]["style"][field] = invalid;
        assert!(Profile::from_json(&bad.to_string()).is_err(), "{field}");
        let direct: Profile = serde_json::from_value(bad["profile"].clone()).unwrap();
        assert!(direct.to_json().is_err());
        assert!(
            direct
                .render_adaptive(&text_kind("?", 72.0), "?", |_| panic!("invalid state"))
                .is_err()
        );
    }
    let mut bad = value.clone();
    bad["profile"]["style"]["recent"][0]["slant"] = serde_json::json!(99);
    assert!(Profile::from_json(&bad.to_string()).is_err());
    let mut bad = value.clone();
    bad["profile"]["style"]["recent"] =
        serde_json::json!(vec![value["profile"]["style"]["recent"][0].clone(); 32]);
    assert!(Profile::from_json(&bad.to_string()).is_err());
    let mut bad = value;
    bad["version"] = 1.into();
    assert!(Profile::from_json(&bad.to_string()).is_err());
}

#[test]
fn adaptive_scratch_ignores_unrelated_full_manual_profile_and_keeps_quotas() {
    let mut profile = Profile::default();
    for ch in (0x100..0x160).map(|v| char::from_u32(v).unwrap()) {
        profile.add_sample(ch, sample(24.0)).unwrap();
    }
    let before = profile.clone();
    let mut calls = Vec::new();
    let kind = text_kind("\u{100}?!?", 72.0);
    assert_core_valid(
        profile
            .render_adaptive(&kind, "\u{100}?!?", |ch| {
                calls.push(ch);
                Ok(fallback_stem())
            })
            .unwrap(),
    );
    assert_eq!(calls, ['!', '?']);
    assert_eq!(profile, before);
    let text: String = (0x200..0x261).map(|v| char::from_u32(v).unwrap()).collect();
    assert!(
        profile
            .render_adaptive(&text_kind(&text, 26.0), &text, |_| panic!(
                "preflight quota"
            ))
            .unwrap_err()
            .contains("96 distinct")
    );
    assert!(
        profile
            .render_adaptive(&text_kind("?", 26.0), "?", |_| Err("no outline".into()))
            .unwrap_err()
            .contains("no outline")
    );
    assert!(
        profile
            .render_adaptive(&text_kind("?", 26.0), "?", |_| Ok(vec![]))
            .is_err()
    );
    let text = "?".repeat(MAX_HANDWRITING_STROKES + 1);
    assert!(
        profile
            .render_adaptive(&text_kind(&text, 26.0), &text, |_| Ok(fallback_stem()))
            .unwrap_err()
            .contains("budget")
    );
    assert_eq!(profile, before);
}

#[test]
fn adaptive_math_uses_layout_leaves_not_semantic_syntax() {
    let profile = Profile::default();
    let kind = ObjectKind::Math {
        position: Point::default(),
        size: 26.0,
        color: Color::default(),
        layout: MathLayout::Fraction(
            Box::new(MathLayout::Text("1".into())),
            Box::new(MathLayout::Radical(Box::new(MathLayout::Text("2".into())))),
        ),
    };
    let mut calls = Vec::new();
    let result = profile
        .render_adaptive(&kind, "1/sqrt(2)", |ch| {
            calls.push(ch);
            Ok(fallback_stem())
        })
        .unwrap();
    assert_eq!(calls, ['1', '2']);
    assert!(
        matches!(&result, ObjectKind::Handwritten { text, layout: Some(_), .. } if text == "1/sqrt(2)")
    );
    assert_core_valid(result);
    assert_eq!(profile, Profile::default());
}

#[test]
fn adaptive_output_admission_is_early_occurrence_weighted_and_variant_exact() {
    let profile = Profile::default();
    let chars: String = (0x200..0x260).map(|v| char::from_u32(v).unwrap()).collect();
    for repetitions in [1, 4] {
        let text = chars.repeat(repetitions);
        let mut calls = 0;
        let error = profile
            .render_adaptive(&text_kind(&text, 26.0), &text, |_| {
                calls += 1;
                let mut ink = fallback_stem();
                ink[0].points = vec![ink[0].points[0]; 512];
                Ok(ink)
            })
            .unwrap_err();
        assert!(error.contains("budget"));
        assert_eq!(calls, MAX_HANDWRITING_POINTS / (512 * repetitions) + 1);
    }
    let mut profile = Profile::default();
    profile.add_sample('x', fallback_stem()).unwrap();
    let mut large = fallback_stem();
    large[0].points = vec![large[0].points[0]; 2048];
    profile.add_sample('x', large).unwrap();
    // Exact cycling fits; charging the largest variant for all occurrences does not.
    let text = format!("{}?", "x".repeat(30));
    assert_core_valid(
        profile
            .render_adaptive(&text_kind(&text, 26.0), &text, |_| Ok(fallback_stem()))
            .unwrap(),
    );
    let text = format!("{}?", "x".repeat(32));
    assert!(
        profile
            .render_adaptive(&text_kind(&text, 26.0), &text, |_| panic!(
                "manual output already too large"
            ))
            .unwrap_err()
            .contains("budget")
    );
}

#[test]
fn rolling_31_statistics_and_f64_sample_times_roundtrip_with_v1_compatibility() {
    let mut profile = Profile::default();
    let mut sample = fallback_stem();
    sample[0].points[0].time = 0.10000000000000002;
    sample[0].points[1].time = 0.8455124082255701;
    profile.add_sample('x', sample).unwrap();
    let mut v1: serde_json::Value = serde_json::from_str(&profile.to_json().unwrap()).unwrap();
    v1["version"] = 1.into();
    assert_eq!(Profile::from_json(&v1.to_string()).unwrap(), profile);
    let mut observations = Vec::new();
    for i in 0..80 {
        let width = 1.0 + (i % 9) as f32 * 0.25;
        assert!(profile.observe_stroke(
            &learning_stroke(7 + i % 11),
            Style {
                width,
                ..Style::default()
            }
        ));
        let json = profile.to_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let recent = value["profile"]["style"]["recent"].as_array().unwrap();
        observations.push(recent.last().unwrap().clone());
        assert_eq!(
            recent,
            &observations[observations.len().saturating_sub(31)..]
        );
        assert_eq!(profile.learned_strokes(), i as u32 + 1);
        let loaded = Profile::from_json(&json).unwrap();
        assert_eq!(loaded, profile);
        assert_eq!(loaded.to_json().unwrap(), json);
        assert_eq!(
            loaded.glyph('x').unwrap().variants[0][0].points[1]
                .time
                .to_bits(),
            0.8455124082255701_f64.to_bits()
        );
    }
}

#[test]
fn profile_unknown_field_policy_is_strict_for_profile_and_compatible_for_core_ink() {
    let mut profile = Profile::default();
    profile.add_sample('x', fallback_stem()).unwrap();
    profile.observe_stroke(&learning_stroke(8), Style::default());
    let original: serde_json::Value = serde_json::from_str(&profile.to_json().unwrap()).unwrap();
    for pointer in [
        "",
        "/profile",
        "/profile/glyphs/0",
        "/profile/style",
        "/profile/style/recent/0",
    ] {
        let mut value = original.clone();
        value.pointer_mut(pointer).unwrap()["future"] = true.into();
        assert!(Profile::from_json(&value.to_string()).is_err(), "{pointer}");
    }
    for pointer in [
        "/profile/glyphs/0/variants/0/0",
        "/profile/glyphs/0/variants/0/0/points/0",
        "/profile/glyphs/0/variants/0/0/style",
        "/profile/glyphs/0/variants/0/0/style/color",
    ] {
        let mut value = original.clone();
        value.pointer_mut(pointer).unwrap()["future"] = true.into();
        assert_eq!(
            Profile::from_json(&value.to_string()).unwrap(),
            profile,
            "{pointer}"
        );
    }
}

#[test]
fn explicit_atomic_save_load_and_bounded_read() {
    let directory =
        std::env::temp_dir().join(format!("board-handwriting-test-{}", board_core::new_id()));
    fs::create_dir(&directory).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(directory.clone());
    let path = directory.join("profile.json");
    let mut profile = Profile::default();
    profile.save(&path).unwrap();
    profile.add_sample('x', sample(24.0)).unwrap();
    profile.save(&path).unwrap();
    assert_eq!(Profile::load(&path).unwrap(), profile);
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    let before = fs::read(&path).unwrap();
    let mut invalid = profile.clone();
    invalid.glyphs[0].variants.clear();
    assert!(invalid.save(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(profile.save(&directory).is_err());
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    File::create(&path)
        .unwrap()
        .set_len(MAX_JSON_BYTES as u64 + 1)
        .unwrap();
    assert!(Profile::load(&path).unwrap_err().contains("8 MiB"));
}
