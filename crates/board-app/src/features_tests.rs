use super::*;
#[test]
fn manual_display_preserves_fraction_radical_plain_text_and_bounds() {
    use board_core::MathLayout::{Fraction, Radical, Row, Text};
    let bounds = board_math::Bounds2D {
        x_min: 0.0,
        x_max: 2.0,
        y_min: 0.0,
        y_max: 2.0,
    };
    for (input, expected) in [
        (
            "1/2+1/3",
            Fraction(Box::new(Text("5".into())), Box::new(Text("6".into()))),
        ),
        (
            "sqrt(1)+sqrt(3)",
            Row(vec![
                Text("1".into()),
                Text(" + ".into()),
                Radical(Box::new(Text("3".into()))),
            ]),
        ),
    ] {
        let output = calculation_display(input, bounds, true).unwrap();
        assert_eq!(output.layout.as_ref(), Some(&expected));
        let kind = output.into_kind(Point::default(), Default::default());
        assert!(matches!(kind, ObjectKind::Math { layout, .. } if layout == expected));
    }
    for (input, prompt) in [("1/2+1/3=", false), ("1/2+1/3", true)] {
        let output = calculation_display(input, bounds, prompt).unwrap();
        assert_eq!(output.text, "5/6");
        assert!(matches!(output.layout, Some(Fraction(..))));
    }
    for prompt in [true, false] {
        let output = calculation_display("sin(1)", bounds, prompt).unwrap();
        assert!(output.text.starts_with("≈ "));
        assert_eq!(output.layout, Some(Text(output.text)));
    }
    for input in ["2x+3=x-1", "x+x", "x^2+y^2=1;x-y=0"] {
        let output = calculation_display(input, bounds, true).unwrap();
        assert_eq!(
            output.text,
            board_math::calculate_with_bounds(input, bounds, Default::default()).unwrap()
        );
        assert!(output.layout.is_none());
        if input == "2x+3=x-1" {
            assert_eq!(output.text, "x = -4");
        }
        assert!(matches!(
            output.into_kind(Point::default(), Default::default()),
            ObjectKind::Text { .. }
        ));
    }
}

#[test]
fn expression_position_and_function_validation() {
    let bounds = board_math::Bounds2D {
        x_min: -10.0,
        x_max: 10.0,
        y_min: -10.0,
        y_max: 10.0,
    };
    assert_eq!(
        calculation_output("1+2=", bounds, false).unwrap(),
        ("3".into(), false)
    );
    assert_eq!(
        calculation_output("1+2", bounds, false).unwrap(),
        ("= 3".into(), false)
    );
    assert_eq!(
        calculation_output("(1+1)", bounds, true).unwrap(),
        ("2".into(), false)
    );
    for input in ["x+1=", "y=", "f(x)=", "(1+1=)", "1+1==", "1+1=2=", "x=1;y="] {
        assert!(calculation_output(input, bounds, false).is_err(), "{input}");
        assert!(board_math::calculate(input).is_err(), "{input}");
    }
    assert!(board_math::calculate("1+1=").is_err());
    assert!(calculation_output("1+1=2", bounds, false).unwrap().1);
    assert!(calculation_output("x+1=2", bounds, false).unwrap().1);
    let quadratic = calculation_output("x^2+y^2=1;x-y=0", bounds, false).unwrap();
    assert!(quadratic.1 && quadratic.0.contains("不保证完备"));
    let b = Rect::from_min_max(Pos2::new(10.0, 20.0), Pos2::new(80.0, 60.0));
    assert_eq!(result_position(b, false), Point { x: 96.0, y: 20.0 });
    assert_eq!(result_position(b, true), Point { x: 10.0, y: 74.0 });
    assert_eq!(function_expression(" y = x ^ 2 "), Some("x ^ 2".into()));
    for input in [
        "y²=x",
        "y-x=1",
        "x²+y²=4",
        "(x-1)^2/4+(y+2)^2/9=1",
        "y=x²",
        "f(x)=x²",
    ] {
        assert_eq!(
            function_expression(input),
            board_math::plot_expression(input)
        );
        assert!(function_expression(input).is_some(), "{input}");
    }
    for input in [
        "2x+3=x-1", "x=2", "x²=2", "x²", "y^3=x", "sin(y)=x", "y=x;x=1",
    ] {
        assert!(function_expression(input).is_none(), "{input}");
    }
    assert!(function_expression(&format!("y={}", "x+".repeat(3000))).is_none());
    assert!(function_expression("x=2").is_none());
    assert!(function_expression("y=???").is_none());
}
#[test]
fn merge_split_is_atomic_and_retains_ranges() {
    let a = plot("x".into(), Point::default());
    let b = plot("x^2".into(), Point::default());
    let ops = merge_plots(&a, &b).unwrap();
    let Operation::Update { object } = &ops[0] else {
        panic!()
    };
    let split = split_plot(object, 1, Point { x: 40.0, y: 50.0 }).unwrap();
    assert_eq!(split.len(), 2);
    assert!(merge_plots(&a, &a).is_none());
    assert!(split_plot(&a, 0, Point::default()).is_none());
}
#[test]
fn merged_plot_intersections_are_deduplicated_and_budgeted() {
    let mut object = plot("x".into(), Point::default());
    if let ObjectKind::FunctionPlot { expressions, .. } = &mut object.kind {
        expressions.push("-x".into());
    }
    assert_eq!(intersections(&object).candidates.len(), 1);
    if let ObjectKind::FunctionPlot { expressions, .. } = &mut object.kind {
        *expressions = vec!["x".into(); 17];
    }
    let report = intersections(&object);
    assert_eq!(report.candidates.len(), 1);
    assert!(report.failed_searches > 0);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.issue == IntersectionIssue::Limit)
    );
    assert_eq!(report.diagnostics.len(), MAX_PLOT_DIAGNOSTICS);
    assert!(report.omitted_diagnostics > 0);
}

#[test]
fn basic_line_plot_intersections_accept_all_plot_spellings() {
    for (expression, expected) in [
        ("x", vec![(0.0, 0.0)]),
        ("y=x", vec![(0.0, 0.0)]),
        ("2*x+1", vec![(-0.5, 0.0), (0.0, 1.0)]),
        ("y=2*x+1", vec![(-0.5, 0.0), (0.0, 1.0)]),
        ("y=2x+1", vec![(-0.5, 0.0), (0.0, 1.0)]),
        ("f(x)=2*x+1", vec![(-0.5, 0.0), (0.0, 1.0)]),
        ("y-x=1", vec![(-1.0, 0.0), (0.0, 1.0)]),
        ("x=2", vec![(2.0, 0.0)]),
    ] {
        let report = intersections(&plot(expression.into(), Point::default()));
        assert!(report.diagnostics.is_empty(), "{expression}: {report:?}");
        assert_eq!(report.candidates.len(), expected.len(), "{expression}");
        for (x, y) in expected {
            let label = format!("({x:.6}, {y:.6})");
            assert!(
                report
                    .candidates
                    .iter()
                    .any(|(_, text)| text.contains(&label)),
                "{expression}: {report:?}"
            );
        }
    }
}

#[test]
fn implicit_and_mixed_intersections_are_unsupported_without_explicit_fallback() {
    for curves in [
        vec!["y^2=x"],
        vec!["x", "y^2=x"],
        vec!["x^2+y^2=1", "-x"],
        vec!["x", "sin(y)=x"],
    ] {
        let mut object = plot(curves[0].into(), Point::default());
        if let ObjectKind::FunctionPlot { expressions, .. } = &mut object.kind {
            *expressions = curves.into_iter().map(String::from).collect();
        }
        let report = intersections_with_options(
            &object,
            board_math::NumericOptions {
                max_evaluations: 0,
                ..Default::default()
            },
        );
        assert!(report.candidates.is_empty());
        assert_eq!(report.failed_searches, 0);
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].issue, IntersectionIssue::Unsupported);
        assert!(report.summary().contains("未执行搜索"));
        assert!(!report.summary().contains("非证明无交点"));
    }
}

#[test]
fn mixed_lines_and_curves_use_the_same_feature_search() {
    for (curves, expected) in [
        (
            vec!["y=x", "y-x=1", "x=2"],
            vec![
                (0.0, 0.0),
                (-1.0, 0.0),
                (0.0, 1.0),
                (2.0, 0.0),
                (2.0, 2.0),
                (2.0, 3.0),
            ],
        ),
        (
            vec!["f(x)=x^2", "x=2"],
            vec![(0.0, 0.0), (2.0, 0.0), (2.0, 4.0)],
        ),
        (
            vec!["y=x^2", "y-x=2"],
            vec![(0.0, 0.0), (-2.0, 0.0), (0.0, 2.0), (-1.0, 1.0), (2.0, 4.0)],
        ),
        (vec!["y=0.1*x+0.3"], vec![(-3.0, 0.0), (0.0, 0.3)]),
    ] {
        let mut object = plot(curves[0].into(), Point { x: 20.0, y: 40.0 });
        if let ObjectKind::FunctionPlot { expressions, .. } = &mut object.kind {
            *expressions = curves.iter().map(|s| (*s).into()).collect();
        }
        let report = intersections(&object);
        assert!(report.diagnostics.is_empty(), "{curves:?}: {report:?}");
        assert_eq!(
            report.candidates.len(),
            expected.len(),
            "{curves:?}: {report:?}"
        );
        for (x, y) in expected {
            let label = format!("({x:.6}, {y:.6})");
            let (point, _) = report
                .candidates
                .iter()
                .find(|(_, text)| text.contains(&label))
                .unwrap_or_else(|| panic!("{label}: {report:?}"));
            let expected = Pos2::new(
                20.0 + ((x + 10.0) / 20.0) as f32 * 500.0,
                40.0 + ((7.0 - y) / 14.0) as f32 * 350.0,
            );
            assert!(point.distance(expected) < 1e-3);
        }
    }
}

#[test]
fn line_feature_search_clips_both_axes_and_distinguishes_parallel_from_coincident() {
    for (curves, non_discrete) in [
        (vec!["x=2", "x=3"], false),
        (vec!["y=x", "y=x+1"], false),
        (vec!["y=x", "y=x+1e-7"], false),
        (vec!["x=2", "2*x=4"], true),
        (vec!["y=x", "2*y-2*x=0"], true),
        (vec!["y=0"], true),
        (vec!["x=0"], true),
    ] {
        let mut object = plot(curves[0].into(), Point::default());
        if let ObjectKind::FunctionPlot { expressions, .. } = &mut object.kind {
            *expressions = curves.iter().map(|s| (*s).into()).collect();
        }
        let report = intersections(&object);
        assert_eq!(
            report.non_discrete_searches > 0,
            non_discrete,
            "{curves:?}: {report:?}"
        );
        assert_eq!(report.failed_searches, 0);
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|d| d.issue == IntersectionIssue::PossiblyNonDiscrete)
        );
    }
    for curves in [
        vec!["x=2", "x=3"],
        vec!["y=x+2", "y=x+3"],
        vec!["x=11"],
        vec!["y=8"],
    ] {
        let mut object = plot(curves[0].into(), Point::default());
        if let ObjectKind::FunctionPlot {
            expressions,
            x_min,
            x_max,
            y_min,
            y_max,
            ..
        } = &mut object.kind
        {
            *expressions = curves.iter().map(|s| (*s).into()).collect();
            (*x_min, *x_max, *y_min, *y_max) = (1.0, 4.0, 1.0, 4.0);
        }
        let report = intersections(&object);
        assert!(report.candidates.is_empty(), "{curves:?}: {report:?}");
        assert!(report.diagnostics.is_empty(), "{report:?}");
    }
}

#[test]
fn intersection_review_boundary_corner_and_original_domain_regressions() {
    let mut object = plot(".1*x+.2".into(), Point::default());
    if let ObjectKind::FunctionPlot {
        expressions,
        x_min,
        x_max,
        y_min,
        y_max,
        ..
    } = &mut object.kind
    {
        expressions.push("x=1".into());
        (*x_min, *x_max, *y_min, *y_max) = (-1.0, 2.0, -1.0, 0.3);
    }
    let report = intersections(&object);
    assert!(report.diagnostics.is_empty(), "{report:?}");
    assert!(
        report
            .candidates
            .iter()
            .any(|(_, label)| label.contains("(1.000000, 0.300000)")),
        "{report:?}"
    );

    if let ObjectKind::FunctionPlot {
        expressions,
        x_min,
        x_max,
        y_min,
        y_max,
        ..
    } = &mut object.kind
    {
        *expressions = vec!["x+y=2".into(), "2*x+2*y=4".into()];
        (*x_min, *x_max, *y_min, *y_max) = (0.0, 1.0, 0.0, 1.0);
    }
    let report = intersections(&object);
    assert_eq!(report.candidates.len(), 1, "{report:?}");
    assert!(report.candidates[0].1.contains("(1.000000, 1.000000)"));
    assert_eq!(report.non_discrete_searches, 0);
    assert!(report.diagnostics.is_empty());

    if let ObjectKind::FunctionPlot {
        expressions,
        x_min,
        x_max,
        y_min,
        y_max,
        ..
    } = &mut object.kind
    {
        *expressions = vec!["y-2*x=1e308*x-1e308*x".into(), "y=x^2".into()];
        (*x_min, *x_max, *y_min, *y_max) = (1.9, 2.1, 3.5, 4.5);
    }
    let report = intersections(&object);
    assert!(report.candidates.is_empty(), "{report:?}");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.issue == IntersectionIssue::DomainGap),
        "{report:?}"
    );
}

#[test]
fn malformed_plot_and_unavailable_solver_have_different_diagnostics() {
    for expression in ["y=", "y=x=1", "f(x)=", "y=x+"] {
        let report = intersections(&plot(expression.into(), Point::default()));
        assert!(report.candidates.is_empty());
        assert!(report.failed_searches > 0, "{expression}: {report:?}");
        assert!(
            report
                .diagnostics
                .iter()
                .all(|d| d.issue == IntersectionIssue::Failed && d.message.contains("语法"))
        );
        assert!(!report.summary().contains("暂不支持"));
    }
    let mut object = plot("x".into(), Point::default());
    if let ObjectKind::FunctionPlot { expressions, .. } = &mut object.kind {
        expressions.extend(vec!["y=".into(); 7]);
        expressions.push("y^2=x".into());
    }
    let report = intersections(&object);
    assert!(report.candidates.is_empty());
    assert!(report.summary().contains("未执行搜索"));
    assert_eq!(report.diagnostics.len(), MAX_PLOT_DIAGNOSTICS);
}

#[test]
fn poles_are_not_intersections() {
    let object = plot("1/x".into(), Point::default());
    let report = intersections(&object);
    assert!(report.candidates.is_empty());
    assert_eq!(report.failed_searches, 0);
    assert!(report.summary().contains("非证明无交点"));
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.issue == IntersectionIssue::DomainGap)
    );
}

#[test]
fn intersection_report_retains_good_points_and_bounds_diagnostics() {
    let mut object = plot("x".into(), Point { x: 20.0, y: 40.0 });
    if let ObjectKind::FunctionPlot { expressions, .. } = &mut object.kind {
        expressions.push("x^2".into());
    }
    let good = intersections(&object);
    assert_eq!(good.candidates.len(), 2);
    assert!(
        good.candidates
            .iter()
            .any(|(_, label)| label.contains("1.000000, 1.000000"))
    );
    assert!(good.noncomplete);
    if let ObjectKind::FunctionPlot { expressions, .. } = &mut object.kind {
        expressions.extend(vec!["无效函数".repeat(100); 14]);
    }
    let mixed = intersections(&object);
    assert_eq!(mixed.candidates, good.candidates);
    assert!(mixed.summary().contains("部分搜索失败"));
    assert_eq!(mixed.diagnostics.len(), MAX_PLOT_DIAGNOSTICS);
    assert!(mixed.omitted_diagnostics > 0);
    assert!(
        mixed
            .diagnostics
            .iter()
            .all(|d| d.message.chars().count() <= 131)
    );
    assert_eq!(plot_text(&"中".repeat(100)).chars().count(), 65);
}

#[test]
fn limited_search_and_coincident_curves_are_explicit() {
    let object = plot("x^2".into(), Point::default());
    let limited = intersections_with_options(
        &object,
        board_math::NumericOptions {
            steps: 128,
            max_evaluations: 32,
            ..Default::default()
        },
    );
    assert_eq!(limited.candidates.len(), 1); // y 轴求值成功，不随根搜索失败丢失。
    assert!(
        limited
            .diagnostics
            .iter()
            .any(|d| d.issue == IntersectionIssue::Limit)
    );
    for expressions in [vec!["0"], vec!["x", "x"], vec!["x-x"]] {
        let mut object = object.clone();
        if let ObjectKind::FunctionPlot {
            expressions: target,
            ..
        } = &mut object.kind
        {
            *target = expressions.iter().map(|s| (*s).into()).collect();
        }
        let report = intersections(&object);
        assert!(report.summary().contains("非离散交点"));
        assert!(!report.summary().contains("非证明无交点"));
        assert!(report.non_discrete_searches > 0);
    }
    let mut object = object;
    if let ObjectKind::FunctionPlot { expressions, .. } = &mut object.kind {
        *expressions = vec!["x".into(), "x+0.0000001".into()];
    }
    assert_eq!(intersections(&object).non_discrete_searches, 0);
}
#[test]
fn ink_button_is_clamped_outside_ink() {
    let viewport = Rect::from_min_size(Pos2::ZERO, egui::vec2(320.0, 240.0));
    for (x, y) in [(80.0, 80.0), (280.0, 0.0), (0.0, 0.0), (280.0, 210.0)] {
        let bounds = Rect::from_min_size(egui::pos2(x, y), egui::vec2(40.0, 30.0));
        let rect = ink_action_rect(bounds, viewport).unwrap();
        assert!(viewport.contains_rect(rect));
        assert!(!rect.intersect(bounds.expand(5.0)).is_positive());
        assert_eq!(rect.size(), egui::vec2(26.0, 26.0));
    }
    assert!(ink_action_rect(viewport, viewport).is_none());
}

#[test]
fn image_button_is_clipped_and_tracks_top_right() {
    let viewport = Rect::from_min_size(Pos2::ZERO, egui::vec2(320.0, 240.0));
    let mut image = BoardObject {
        id: "i".into(),
        kind: ObjectKind::Image {
            position: Point { x: 260.0, y: 10.0 },
            width: 200.0,
            height: 100.0,
            asset_ref: "asset:a".into(),
        },
    };
    let rect = image_agent_rect(&image, viewport).unwrap();
    assert!(viewport.contains_rect(rect));
    assert_eq!(rect.right_top(), egui::pos2(320.0, 10.0));
    if let ObjectKind::Image { position, .. } = &mut image.kind {
        position.x = 400.0;
    }
    assert!(image_agent_rect(&image, viewport).is_none());
}

#[test]
fn background_slot_does_not_queue_or_block_ui() {
    let mut worker = Background::default();
    let (tx, rx) = std::sync::mpsc::channel();
    worker.start(egui::Context::default(), move || {
        rx.recv().unwrap();
        7
    });
    worker.start(egui::Context::default(), || panic!("must not queue"));
    assert!(worker.busy());
    assert!(worker.take().is_none());
    tx.send(()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if let Some(result) = worker.take() {
            assert_eq!(result.unwrap(), 7);
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(!worker.busy());
}

fn drain<T: Send + 'static>(worker: &mut Background<T>) -> Option<Result<T, String>> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while worker.busy() {
        let result = worker.take();
        if result.is_some() {
            return result;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    None
}

#[test]
fn cancelled_panicking_work_drains_without_overwriting_or_queueing() {
    let mut worker = Background::<usize>::default();
    let (tx, rx) = std::sync::mpsc::channel();
    worker.start(egui::Context::default(), move || {
        rx.recv().unwrap();
        panic!("simulated worker failure")
    });
    worker.cancel();
    worker.start(egui::Context::default(), || panic!("must not queue"));
    assert!(worker.busy());
    tx.send(()).unwrap();
    assert!(drain(&mut worker).is_none());
    worker.start(egui::Context::default(), || 42);
    assert_eq!(drain(&mut worker).unwrap().unwrap(), 42);
}

#[test]
fn panic_and_disconnected_channel_release_the_slot() {
    let mut worker = Background::<usize>::default();
    worker.start(egui::Context::default(), || panic!("simulated failure"));
    assert!(drain(&mut worker).unwrap().is_err());
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    worker.receiver = Some(rx);
    drop(tx);
    assert!(worker.take().unwrap().is_err());
    assert!(!worker.busy());
}

#[test]
fn dropping_receiver_during_work_does_not_panic_on_send() {
    let mut worker = Background::default();
    let ctx = egui::Context::default();
    let (paint_tx, paint_rx) = std::sync::mpsc::channel();
    ctx.set_request_repaint_callback(move |_| {
        let _ = paint_tx.send(());
    });
    let (tx, rx) = std::sync::mpsc::channel();
    worker.start(ctx, move || rx.recv().unwrap());
    drop(worker);
    tx.send(7).unwrap();
    // repaint 在发送结果之后发生，验证发送失败路径已经正常走完。
    paint_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
}

#[test]
fn rejects_invalid_png() {
    assert!(decode_png(b"not png").is_err());
}
#[test]
fn passthrough_fails_closed_outside_monitor_and_near_controls() {
    let viewport = Rect::from_min_size(Pos2::ZERO, egui::vec2(320.0, 240.0));
    let controls = [Rect::from_min_size(
        egui::pos2(20.0, 180.0),
        egui::vec2(100.0, 30.0),
    )];
    assert!(passthrough_at(
        egui::pos2(200.0, 100.0),
        viewport,
        &controls
    ));
    assert!(!passthrough_at(
        egui::pos2(-10.0, 100.0),
        viewport,
        &controls
    ));
    assert!(!passthrough_at(egui::pos2(200.0, 100.0), viewport, &[]));
    assert!(!passthrough_at(
        egui::pos2(15.0, 180.0),
        viewport,
        &controls
    ));
    assert!(!passthrough_at(
        egui::pos2(f32::NAN, 100.0),
        viewport,
        &controls
    ));
}

#[test]
fn dpi_negative_monitor_and_expression_grouping() {
    assert_eq!(
        cursor_logical((-1600, 300), (-1920, 0), 2.0),
        Pos2::new(160.0, 150.0)
    );
    let stroke = |x, y| {
        vec![
            StrokePoint {
                x,
                y,
                time: 0.0,
                pressure: 1.0,
            },
            StrokePoint {
                x: x + 20.0,
                y: y + 30.0,
                time: 1.0,
                pressure: 1.0,
            },
        ]
    };
    assert!(joins_expression(&[stroke(0.0, 0.0)], &stroke(45.0, 0.0)));
    assert!(!joins_expression(&[stroke(0.0, 0.0)], &stroke(45.0, 200.0)));
}
#[test]
fn confirmation_cancel_and_revision_protection() {
    use board_hwr::{AutoCalculate, ContextToken};
    let document = board_core::Document::new();
    let context = ContextToken::capture(&document);
    let strokes = vec![vec![StrokePoint {
        x: 0.0,
        y: 0.0,
        time: 0.0,
        pressure: 1.0,
    }]];
    let now = std::time::Instant::now();
    let mut gate = AutoCalculate::new();
    gate.set_enabled(true);
    gate.strokes_finished(now, context.clone(), &strokes)
        .unwrap();
    assert!(
        gate.poll(now + std::time::Duration::from_millis(2499), &context)
            .is_none()
    );
    let request = gate
        .poll(now + AutoCalculate::IDLE_DELAY, &context)
        .unwrap();
    let mut changed = context.clone();
    changed.revision += 1;
    assert!(!gate.confirm(&request, &changed));
    gate.strokes_finished(now, changed.clone(), &strokes)
        .unwrap();
    let request = gate
        .poll(now + AutoCalculate::IDLE_DELAY, &changed)
        .unwrap();
    gate.set_enabled(false);
    assert!(!gate.confirm(&request, &changed));
}
#[test]
fn snaps_to_segment_and_skips_self() {
    let line = BoardObject {
        id: "line".into(),
        kind: ObjectKind::Shape {
            shape: board_core::ShapeKind::Line,
            points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 100.0, y: 0.0 }],
            style: Default::default(),
        },
    };
    let page = Page {
        id: "p".into(),
        objects: vec![line],
    };
    assert_eq!(
        snap_endpoint(&page, Point { x: 50.0, y: 5.0 }, None),
        Point { x: 50.0, y: 0.0 }
    );
    assert_eq!(
        snap_endpoint(&page, Point { x: 50.0, y: 5.0 }, Some("line")),
        Point { x: 50.0, y: 5.0 }
    );
}
