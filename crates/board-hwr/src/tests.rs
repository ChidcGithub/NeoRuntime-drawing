use super::*;
use std::time::{Duration, Instant};

fn ink(label: &str, x: f32, y: f32, scale: f32) -> Vec<Vec<StrokePoint>> {
    let mut lines = templates::builtins()
        .into_iter()
        .find(|t| t.text == label)
        .unwrap()
        .strokes;
    for p in lines.iter_mut().flatten() {
        p.x = p.x * scale + x;
        p.y = p.y * scale + y;
    }
    lines
}
fn line(points: &[(f32, f32)]) -> Vec<StrokePoint> {
    points
        .iter()
        .map(|&(x, y)| StrokePoint {
            x,
            y,
            time: 0.0,
            pressure: 0.5,
        })
        .collect()
}
fn token(revision: u64) -> ContextToken {
    ContextToken {
        document_id: "doc".into(),
        page_id: "page".into(),
        revision,
    }
}
fn enabled() -> AutoCalculate {
    let mut g = AutoCalculate::new();
    g.set_enabled(true);
    g
}

#[test]
fn every_builtin_survives_translation_scale_and_reversed_strokes() {
    let recognizer = InkRecognizer::default();
    for template in recognizer.templates() {
        let mut transformed = template.strokes.clone();
        for stroke in &mut transformed {
            for p in stroke.iter_mut() {
                p.x = p.x * 73.0 + 240.0;
                p.y = p.y * 73.0 - 80.0;
            }
            stroke.reverse();
        }
        transformed.reverse();
        let candidates = recognizer.recognize_candidates(&transformed).unwrap();
        assert!(
            candidates
                .iter()
                .any(|c| c.text == template.text && c.confidence > 0.7),
            "{}: {candidates:?}",
            template.text
        );
        let result = recognizer.recognize(&transformed).unwrap();
        assert!(
            result.candidates.iter().any(|c| c.text == template.text),
            "{}: {result:?}",
            template.text
        );
        assert!(result.requires_confirmation);
    }
}

#[test]
fn nonuniform_sampling_and_small_handwriting_perturbations() {
    let mut strokes = ink("2", 0.0, 0.0, 100.0);
    let original = strokes[0].clone();
    strokes[0].clear();
    for (i, pair) in original.windows(2).enumerate() {
        for j in 0..(i + 2) {
            let t = j as f32 / (i + 2) as f32;
            strokes[0].push(StrokePoint {
                x: pair[0].x + (pair[1].x - pair[0].x) * t + (i as f32).sin() * 0.8,
                y: pair[0].y + (pair[1].y - pair[0].y) * t,
                time: 0.0,
                pressure: 0.5,
            });
        }
    }
    strokes[0].push(*original.last().unwrap());
    let result = recognize(&strokes).unwrap();
    assert_eq!(result.text, "2");
    assert!(result.confidence > 0.8);
}

#[test]
fn multistroke_expression_sorted_spatially_not_by_input_order() {
    let mut strokes = ink("=", 180.0, 0.0, 40.0);
    strokes.extend(ink("2", 0.0, 0.0, 40.0));
    strokes.extend(ink("+", 60.0, 0.0, 40.0));
    strokes.extend(ink("4", 120.0, 0.0, 40.0));
    let result = recognize(&strokes).unwrap();
    assert_eq!(result.text, "2+4=");
}

#[test]
fn fraction_and_exponent_layout_not_division_confusion() {
    let mut fraction = ink("1", 35.0, 0.0, 30.0);
    fraction.push(line(&[(0.0, 40.0), (90.0, 40.0)]));
    fraction.extend(ink("2", 20.0, 50.0, 30.0));
    let result = recognize(&fraction).unwrap();
    assert_eq!(result.text, "(1)/(2)");
    assert!(result.confidence <= 0.82);
    let mut exponent = ink("2", 0.0, 30.0, 60.0);
    exponent.extend(ink("3", 65.0, 0.0, 25.0));
    exponent.extend(ink("+", 110.0, 30.0, 60.0));
    exponent.extend(ink("1", 190.0, 30.0, 60.0));
    assert_eq!(recognize(&exponent).unwrap().text, "2^(3)+1");
    assert_eq!(recognize(&ink("÷", 0.0, 0.0, 50.0)).unwrap().text, "÷");
}

#[test]
fn ambiguous_cross_has_both_candidates_and_lower_confidence() {
    let result = recognize(&ink("x", 0.0, 0.0, 40.0)).unwrap();
    assert!(result.candidates.iter().any(|c| c.text == "x"));
    assert!(result.candidates.iter().any(|c| c.text == "×"));
    assert!(result.confidence < 0.8);
    assert!(result.requires_confirmation);
}

#[test]
fn empty_invalid_and_unfamiliar_shapes_reject() {
    assert!(matches!(recognize(&[]), Err(Error::EmptyInput)));
    assert!(matches!(recognize(&[vec![]]), Err(Error::EmptyInput)));
    let mut invalid = ink("2", 0.0, 0.0, 10.0);
    invalid[0][0].x = f32::NAN;
    assert!(matches!(recognize(&invalid), Err(Error::InvalidInput(_))));
    let zigzag = vec![line(&[
        (0.0, 0.0),
        (1.0, 1.0),
        (0.0, 2.0),
        (1.0, 3.0),
        (0.0, 4.0),
        (1.0, 5.0),
        (0.0, 6.0),
    ])];
    assert!(matches!(recognize(&zigzag), Err(Error::Rejected)));
    let overdraw = vec![line(&[
        (0.0, 0.0),
        (10.0, 0.0),
        (0.0, 0.0),
        (10.0, 0.0),
        (0.0, 0.0),
    ])];
    assert!(matches!(recognize(&overdraw), Err(Error::Rejected)));
    assert!(
        InkRecognizer::empty()
            .recognize_candidates(&ink("2", 0.0, 0.0, 10.0))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn learning_and_json_roundtrip_and_transactional_import() {
    let strokes = vec![line(&[(0.0, 0.0), (1.0, 1.0), (0.0, 2.0), (1.0, 3.0)])];
    let mut recognizer = InkRecognizer::empty();
    recognizer.register_template("z", &strokes).unwrap();
    let json = recognizer.to_json().unwrap();
    let mut loaded = InkRecognizer::from_json(&json).unwrap();
    assert_eq!(loaded.recognize(&strokes).unwrap().text, "z");
    assert_eq!(loaded.templates(), recognizer.templates());
    assert!(
        loaded
            .import_json("{\"version\":2,\"templates\":[]}")
            .is_err()
    );
    assert_eq!(loaded.templates(), recognizer.templates());
    assert!(loaded.register_template("bad label", &strokes).is_err());
    assert!(loaded.register_template("z", &[]).is_err());
    let all = InkRecognizer::default();
    assert_eq!(
        InkRecognizer::from_json(&all.to_json().unwrap())
            .unwrap()
            .templates(),
        all.templates()
    );
}

#[test]
fn json_save_and_load() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!(".hwr-test-{}.json", board_core::new_id()));
    let recognizer = InkRecognizer::default();
    recognizer.save(&path).unwrap();
    let loaded = InkRecognizer::load(&path);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(loaded.unwrap().templates(), recognizer.templates());
}

// Independently authored fixtures: not fetched from the recognition vocabulary.
fn handwritten_sin() -> Vec<Vec<StrokePoint>> {
    vec![
        line(&[
            (27.0, 6.0),
            (18.0, 2.0),
            (4.0, 8.0),
            (6.0, 17.0),
            (23.0, 25.0),
            (26.0, 35.0),
            (13.0, 41.0),
            (1.0, 37.0),
        ]),
        line(&[(42.0, 14.0), (42.5, 40.0)]),
        line(&[(42.0, 1.0), (42.3, 1.5)]),
        line(&[
            (59.0, 40.0),
            (59.5, 1.0),
            (60.0, 15.0),
            (69.0, 4.0),
            (79.0, 2.0),
            (86.0, 12.0),
            (87.0, 40.0),
        ]),
    ]
}

fn transformed_noisy(strokes: &[Vec<StrokePoint>], scale: f32) -> Vec<Vec<StrokePoint>> {
    let mut out = strokes.to_vec();
    for (i, stroke) in out.iter_mut().enumerate() {
        for (j, p) in stroke.iter_mut().enumerate() {
            p.x = (p.x + ((i * 7 + j) as f32).sin() * 0.13) * scale + 350.0;
            p.y = (p.y + ((i + j * 3) as f32).cos() * 0.13) * scale - 90.0;
            p.pressure = (j % 11) as f32 / 10.0;
            p.time = (j * 37) as f64;
        }
        stroke.reverse();
    }
    out.reverse();
    out
}

#[test]
fn independent_function_ink_with_noise_order_scale_and_pressure_changes() {
    let mut expression = vec![
        line(&[(0.0, 0.0), (19.0, 21.0)]),
        line(&[(36.0, 1.0), (20.0, 21.0), (8.0, 41.0)]),
        line(&[(52.0, 10.0), (91.0, 10.5)]),
        line(&[(53.0, 30.0), (92.0, 30.0)]),
    ];
    let mut sin = handwritten_sin();
    for p in sin.iter_mut().flatten() {
        p.x += 110.0;
    }
    expression.extend(sin);
    expression.extend([
        line(&[
            (219.0, 0.0),
            (208.0, 11.0),
            (204.0, 21.0),
            (208.0, 31.0),
            (220.0, 41.0),
        ]),
        line(&[(235.0, 1.0), (274.0, 40.0)]),
        line(&[(275.0, 0.0), (236.0, 41.0)]),
        line(&[
            (289.0, 0.0),
            (301.0, 11.0),
            (305.0, 20.0),
            (301.0, 30.0),
            (289.0, 40.0),
        ]),
    ]);
    for scale in [0.4, 1.0, 3.7] {
        let input = transformed_noisy(&expression, scale);
        let result = recognize(&input).unwrap();
        assert!(
            result.candidates.iter().any(|c| c.text == "y=sin(x)"),
            "{result:?}"
        );
        assert!(result.requires_confirmation);
        let mut pressure_only = input.clone();
        for p in pressure_only.iter_mut().flatten() {
            p.pressure = 1.0;
            p.time = 0.0;
        }
        assert_eq!(result, recognize(&pressure_only).unwrap());
    }
}

#[test]
fn function_tokens_compose_without_correcting_ambiguous_digits() {
    for (letters, expected) in [
        (vec!["s", "i", "n"], "sin"),
        (vec!["c", "o", "s"], "cos"),
        (vec!["t", "a", "n"], "tan"),
        (vec!["l", "o", "g"], "log"),
        (vec!["l", "n"], "ln"),
    ] {
        let mut strokes = Vec::new();
        for (i, letter) in letters.iter().enumerate() {
            strokes.extend(ink(letter, i as f32 * 60.0, 0.0, 40.0));
        }
        let result = recognize(&strokes).unwrap();
        assert!(
            result.candidates.iter().any(|c| c.text == expected),
            "{result:?}"
        );
        if expected == "cos" || expected == "log" {
            assert!(result.candidates.iter().any(|c| c.text.contains('0')));
            assert!(result.confidence < 0.8);
        }
    }
}

#[test]
fn handwritten_square_root_and_function_power() {
    let radical = vec![
        line(&[(0.0, 31.0), (11.0, 50.0), (31.0, 0.0), (104.0, 1.0)]),
        line(&[
            (47.0, 21.0),
            (54.0, 14.0),
            (71.0, 14.0),
            (78.0, 21.0),
            (75.0, 27.0),
            (47.0, 45.0),
            (79.0, 45.0),
        ]),
    ];
    for scale in [0.5, 2.3] {
        let result = recognize(&transformed_noisy(&radical, scale)).unwrap();
        assert_eq!(result.text, "sqrt(2)");
        assert!(result.confidence <= 0.8);
    }
    let mut power = ink("y", 0.0, 30.0, 60.0);
    power.extend(ink("=", 80.0, 30.0, 60.0));
    power.extend(ink("x", 160.0, 30.0, 60.0));
    power.push(line(&[
        (225.0, 6.0),
        (230.0, 1.0),
        (241.0, 1.0),
        (246.0, 6.0),
        (244.0, 11.0),
        (225.0, 26.0),
        (249.0, 26.0),
    ]));
    let result = recognize(&power).unwrap();
    assert!(
        result.candidates.iter().any(|c| c.text == "y=x^(2)"),
        "{result:?}"
    );
    assert!(result.requires_confirmation);
}

#[test]
fn learned_disjoint_function_tokens_survive_json_and_layout() {
    let mut recognizer = InkRecognizer::empty();
    let mut word = handwritten_sin();
    // Five strokes exceeds the old symbol limit, and remains spatially disjoint.
    word.push(line(&[(96.0, 0.0), (98.0, 20.0), (96.0, 40.0)]));
    for label in ["sin", "cos", "tan", "log", "ln", "sqrt"] {
        recognizer = InkRecognizer::empty();
        recognizer.register_template(label, &word).unwrap();
        let loaded = InkRecognizer::from_json(&recognizer.to_json().unwrap()).unwrap();
        let result = loaded.recognize(&transformed_noisy(&word, 1.7)).unwrap();
        assert_eq!(result.text, label);
        assert!(result.requires_confirmation);
    }
    recognizer.register_template("sin", &word).unwrap();
    let candidates = recognizer.recognize_candidates(&word).unwrap();
    assert_eq!(candidates.len(), 2);
    assert!(candidates.iter().all(|c| c.confidence < 0.8));
}

fn equation(y: f32, minus: bool) -> Vec<Vec<StrokePoint>> {
    let mut strokes = ink("x", 0.0, y, 40.0);
    strokes.extend(ink(
        if minus { "-" } else { "+" },
        60.0,
        y + if minus { 20.0 } else { 0.0 },
        40.0,
    ));
    strokes.extend(ink("y", 120.0, y, 40.0));
    strokes.extend(ink("=", 180.0, y, 40.0));
    strokes.extend(ink(if minus { "1" } else { "2" }, 240.0, y, 40.0));
    strokes
}

#[test]
fn equation_rows_and_explicit_semicolon_preserve_system_separator() {
    let mut rows = equation(0.0, false);
    rows.extend(equation(100.0, true));
    rows.reverse();
    let result = recognize(&rows).unwrap();
    assert!(
        result.candidates.iter().any(|c| c.text == "x+y=2;x-y=1"),
        "{result:?}"
    );
    assert!(result.confidence <= 0.8);
    let mut inline = equation(0.0, false);
    inline.extend(ink(";", 300.0, 0.0, 40.0));
    let mut second = equation(0.0, true);
    for p in second.iter_mut().flatten() {
        p.x += 340.0;
    }
    inline.extend(second);
    let result = recognize(&inline).unwrap();
    assert!(
        result.candidates.iter().any(|c| c.text == "x+y=2;x-y=1"),
        "{result:?}"
    );
}

#[test]
fn independent_equation_rows_survive_noise_and_reordered_ink() {
    let strokes = vec![
        line(&[(0.0, 0.0), (19.0, 21.0)]),
        line(&[(37.0, 0.0), (20.0, 20.0), (8.0, 41.0)]),
        line(&[(57.0, 11.0), (98.0, 10.0)]),
        line(&[(57.0, 30.0), (97.0, 30.0)]),
        line(&[(121.0, 0.0), (121.5, 40.0)]),
        line(&[(1.0, 109.0), (40.0, 150.0)]),
        line(&[(40.0, 110.0), (0.0, 150.0)]),
        line(&[(58.0, 120.0), (98.0, 120.5)]),
        line(&[(57.0, 140.0), (98.0, 140.0)]),
        line(&[
            (119.0, 118.0),
            (128.0, 110.0),
            (147.0, 111.0),
            (155.0, 119.0),
            (152.0, 126.0),
            (120.0, 150.0),
            (159.0, 150.0),
        ]),
    ];
    for scale in [0.3, 2.6] {
        let result = recognize(&transformed_noisy(&strokes, scale)).unwrap();
        assert!(
            result.candidates.iter().any(|c| c.text == "y=1;x=2"),
            "{result:?}"
        );
        assert!(result.requires_confirmation);
    }
}

#[test]
fn independent_print_constants_and_function_letter() {
    let fixtures = [
        (
            "f",
            vec![
                line(&[(27.0, 3.0), (19.0, 1.0), (10.0, 9.0), (11.0, 42.0)]),
                line(&[(1.0, 17.0), (27.0, 16.0)]),
            ],
        ),
        (
            "e",
            vec![line(&[
                (1.0, 19.0),
                (28.0, 18.0),
                (24.0, 7.0),
                (15.0, 1.0),
                (5.0, 7.0),
                (0.0, 20.0),
                (4.0, 35.0),
                (17.0, 41.0),
                (29.0, 35.0),
            ])],
        ),
        (
            "pi",
            vec![
                line(&[(0.0, 7.0), (41.0, 6.0)]),
                line(&[(10.0, 7.0), (8.0, 40.0)]),
                line(&[(31.0, 7.0), (30.0, 35.0), (37.0, 40.0)]),
            ],
        ),
    ];
    for (label, strokes) in fixtures {
        let result = recognize(&transformed_noisy(&strokes, 2.0)).unwrap();
        assert!(
            result.candidates.iter().any(|c| c.text == label),
            "{result:?}"
        );
    }
}

#[test]
fn incomplete_equation_row_is_rejected_instead_of_appended() {
    let mut strokes = equation(0.0, false);
    // The lower ink has no equals sign and disjoint x projection: previously it
    // could be appended to the first row despite being far below the baseline.
    strokes.extend(ink("2", 340.0, 130.0, 40.0));
    assert!(matches!(recognize(&strokes), Err(Error::Rejected)));
}

#[test]
fn malformed_radicals_and_unknown_ink_are_not_completed() {
    let roof = line(&[(0.0, 30.0), (11.0, 50.0), (30.0, 0.0), (110.0, 0.0)]);
    assert!(matches!(
        recognize(std::slice::from_ref(&roof)),
        Err(Error::Rejected)
    ));
    let mut crossing_roof = vec![roof.clone()];
    crossing_roof.extend(ink("2", 45.0, -10.0, 40.0));
    assert!(matches!(recognize(&crossing_roof), Err(Error::Rejected)));
    let mut unknown_body = vec![roof];
    unknown_body.push(line(&[
        (45.0, 10.0),
        (85.0, 40.0),
        (45.0, 40.0),
        (85.0, 10.0),
        (45.0, 10.0),
        (85.0, 40.0),
    ]));
    assert!(matches!(recognize(&unknown_body), Err(Error::Rejected)));
    let mut function = handwritten_sin();
    function.push(line(&[
        (105.0, 0.0),
        (130.0, 40.0),
        (105.0, 0.0),
        (130.0, 40.0),
        (105.0, 0.0),
    ]));
    assert!(matches!(recognize(&function), Err(Error::Rejected)));
}

#[test]
fn gate_boundary_once_confirmation_and_duplicate_snapshot() {
    let mut gate = enabled();
    let now = Instant::now();
    let t = token(7);
    let strokes = ink("2", 0.0, 0.0, 40.0);
    gate.strokes_finished(now, t.clone(), &strokes).unwrap();
    assert!(gate.poll(now - Duration::from_millis(1), &t).is_none());
    assert!(gate.poll(now + Duration::from_millis(2499), &t).is_none());
    let request = gate.poll(now + Duration::from_millis(2500), &t).unwrap();
    assert_eq!(request.context, t);
    assert_eq!(request.strokes, strokes);
    assert!(gate.poll(now + Duration::from_secs(9), &t).is_none());
    assert!(gate.confirm(&request, &t));
    assert!(!gate.confirm(&request, &t));
    gate.strokes_finished(now + Duration::from_secs(10), t.clone(), &strokes)
        .unwrap();
    assert!(gate.poll(now + Duration::from_secs(20), &t).is_none());
}

#[test]
fn continued_ink_cancels_old_worker_and_restarts_timer() {
    let now = Instant::now();
    let mut gate = enabled();
    let mut strokes = ink("2", 0.0, 0.0, 40.0);
    gate.strokes_finished(now, token(1), &strokes).unwrap();
    let request = gate.poll(now + Duration::from_secs(3), &token(1)).unwrap();
    gate.input_started();
    assert!(!gate.confirm(&request, &token(1)));
    strokes.extend(ink("+", 50.0, 0.0, 40.0));
    gate.strokes_finished(now + Duration::from_secs(4), token(2), &strokes)
        .unwrap();
    assert!(gate.poll(now + Duration::from_secs(6), &token(2)).is_none());
    assert!(
        gate.poll(now + Duration::from_millis(6500), &token(2))
            .is_some()
    );
}

#[test]
fn document_page_revision_and_disabled_gate_cancel() {
    let now = Instant::now();
    let strokes = ink("2", 0.0, 0.0, 40.0);
    for changed in [
        ContextToken {
            document_id: "other".into(),
            ..token(1)
        },
        ContextToken {
            page_id: "other".into(),
            ..token(1)
        },
        token(2),
    ] {
        let mut gate = enabled();
        gate.strokes_finished(now, token(1), &strokes).unwrap();
        let request = gate.poll(now + Duration::from_secs(3), &token(1)).unwrap();
        gate.context_changed(&changed);
        assert!(!gate.confirm(&request, &token(1)));
        assert!(gate.poll(now + Duration::from_secs(6), &token(1)).is_none());
    }
    let mut gate = AutoCalculate::new();
    gate.strokes_finished(now, token(1), &strokes).unwrap();
    assert!(gate.poll(now + Duration::from_secs(3), &token(1)).is_none());
    gate.set_enabled(true);
    gate.strokes_finished(now, token(1), &strokes).unwrap();
    let request = gate.poll(now + Duration::from_secs(3), &token(1)).unwrap();
    gate.set_enabled(false);
    assert!(!gate.confirm(&request, &token(1)));
}

#[test]
fn gate_rejects_modified_request_and_stale_confirmation() {
    let now = Instant::now();
    let mut gate = enabled();
    gate.strokes_finished(now, token(1), &ink("2", 0.0, 0.0, 40.0))
        .unwrap();
    let request = gate.poll(now + Duration::from_secs(3), &token(1)).unwrap();
    let mut modified = request.clone();
    modified.strokes[0][0].x += 1.0;
    assert!(!gate.confirm(&modified, &token(1)));
    assert!(!gate.confirm(&request, &token(2)));
    assert!(!gate.confirm(&request, &token(1)));
}

#[test]
fn duplicate_penup_notification_preserves_original_deadline() {
    let now = Instant::now();
    let mut gate = enabled();
    let strokes = ink("1", 0.0, 0.0, 50.0);
    gate.strokes_finished(now, token(1), &strokes).unwrap();
    gate.strokes_finished(now + Duration::from_secs(1), token(1), &strokes)
        .unwrap();
    assert!(
        gate.poll(now + Duration::from_millis(2500), &token(1))
            .is_some()
    );
}
