#[test]
fn real_ink_waits_for_pointer_click_and_undo_is_one_item() {
    let mut app = app();
    assert!(app.ink_math_mode == InkMathMode::Off);
    assert!(app.neural_recognizer.is_none());
    assert!(!app.model_loader.busy());
    app.set_ink_math_mode(InkMathMode::Confirm);
    let now = Instant::now();
    let count = write_sample(&mut app, now);
    let strokes = app.ink.clone();
    let recognized = app.recognizer.recognize(&strokes).unwrap();
    assert_eq!(recognized.text, "1+1", "{recognized:?}");
    assert!(recognized.confidence >= 0.9, "{recognized:?}");
    assert!(recognized.requires_confirmation);
    let ctx = egui::Context::default();
    app.poll_ink(&ctx, now + Duration::from_millis(2499), |_| panic!("early"));
    assert!(!app.hwr_worker.busy());
    let recognizer = app.recognizer.clone();
    app.poll_ink(&ctx, now + AutoCalculate::IDLE_DELAY, move |strokes| {
        recognizer.recognize(strokes).map_err(|e| e.to_string())
    });
    drain_workers(&mut app);
    for _ in 0..100 {
        app.poll_workers(&ctx);
        app.poll_ink(&ctx, now + Duration::from_secs(20), |_| panic!("duplicate"));
        assert!(!app.math_worker.busy());
        assert_eq!(app.session.document.current_page().objects.len(), count);
    }
    assert!(!app.math);
    assert_eq!(app.ink_action().unwrap().1, "=");
    click_ink_icon(&mut app, &ctx);
    assert!(app.ink_action_busy());
    click_ink_icon(&mut app, &ctx);
    assert_eq!(app.session.document.current_page().objects.len(), count);
    app.confirm_calculation(&ctx);
    drain_workers(&mut app);
    assert_eq!(app.session.document.current_page().objects.len(), count + 1);
    let ObjectKind::Math {
        layout, position, ..
    } = &app
        .session
        .document
        .current_page()
        .objects
        .last()
        .unwrap()
        .kind
    else {
        panic!()
    };
    assert_eq!(features::layout_text(layout), "= 2");
    assert_eq!(
        *position,
        features::result_position(features::ink_bounds(&strokes), false)
    );
    assert!(app.calculation.is_none());
    app.poll_ink(&ctx, now + Duration::from_secs(5), |_| panic!("duplicate"));
    drain_workers(&mut app);
    assert_eq!(app.session.document.current_page().objects.len(), count + 1);
    app.undo(false);
    assert_eq!(app.session.document.current_page().objects.len(), count);
    app.undo(true);
    assert_eq!(app.session.document.current_page().objects.len(), count + 1);
}

#[test]
fn off_confirm_weak_ambiguous_and_unsupported_wait_for_click() {
    let ctx = egui::Context::default();
    for mode in [InkMathMode::Off, InkMathMode::Confirm] {
        let mut app = app();
        app.set_ink_math_mode(mode);
        let now = Instant::now();
        let count = write_sample(&mut app, now);
        app.poll_ink(&ctx, now + AutoCalculate::IDLE_DELAY, |_| {
            Ok(candidate("1+1", 0.99))
        });
        drain_workers(&mut app);
        assert_eq!(app.session.document.current_page().objects.len(), count);
        if mode == InkMathMode::Confirm {
            assert!(app.recognition.is_some());
            app.expression = "1+2".into();
            app.confirm_calculation(&ctx);
            drain_workers(&mut app);
            assert_eq!(app.math_result, "= 3");
        }
    }
    let mut ambiguous = candidate("1+1", 0.99);
    ambiguous.candidates.push(board_hwr::Candidate {
        text: "1-1".into(),
        confidence: 0.91,
    });
    for recognition in [
        candidate("1+1", 0.7),
        ambiguous,
        candidate("1/0", 0.99),
        candidate("x^3+y=1;x-y=0", 0.99),
    ] {
        let mut app = app();
        app.set_ink_math_mode(InkMathMode::Confirm);
        let now = Instant::now();
        let count = write_sample(&mut app, now);
        app.poll_ink(&ctx, now + AutoCalculate::IDLE_DELAY, move |_| {
            Ok(recognition)
        });
        drain_workers(&mut app);
        assert_eq!(app.session.document.current_page().objects.len(), count);
        assert!(app.calculation.is_some());
        assert!(app.recognition.is_some());
        assert!(app.math_result.contains("纠错"), "{}", app.math_result);
        // Errors leave the gate available for an explicit corrected confirmation.
        app.expression = "2+3".into();
        app.confirm_calculation(&ctx);
        drain_workers(&mut app);
        assert_eq!(app.math_result, "= 5");
    }
}

#[test]
fn manual_plot_pointer_classifies_raw_expression_and_full_equation() {
    for (input, expected) in [
        ("x²", "x^2"),
        (" y = x² ", "x^2"),
        ("y²=x", "y^2=x"),
        ("y-x=1", "y-x=1"),
    ] {
        let mut app = app();
        app.math = true;
        app.expression = input.into();
        let ctx = egui::Context::default();
        let mut panel = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(1600.0, 1600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |_| app.panels(&ctx),
            );
            output.textures_delta.clear();
            output
        };
        panel(vec![]);
        let output = panel(vec![]);
        let pos = text_position(&output, "绘制函数");
        panel(vec![egui::Event::PointerMoved(pos), pointer(pos, true)]);
        panel(vec![pointer(pos, false)]);
        assert_eq!(app.session.document.current_page().objects.len(), 1);
        assert!(
            matches!(&app.session.document.current_page().objects[0].kind, ObjectKind::FunctionPlot { expressions, .. } if expressions == &[expected])
        );
        app.undo(false);
        assert!(app.session.document.current_page().objects.is_empty());
    }
}

#[test]
fn text_equation_plot_pointer_keeps_full_relation() {
    for input in ["y²=x", "y-x=1", "x²+y²=4", "y=x²"] {
        let mut app = app();
        app.add(ObjectKind::Text {
            position: Point { x: 100.0, y: 100.0 },
            text: input.into(),
            size: 26.0,
            color: Color::default(),
        });
        let ctx = egui::Context::default();
        plot_frame(&mut app, &ctx, vec![]);
        plot_frame(&mut app, &ctx, vec![]);
        assert_eq!(app.controls.len(), 1);
        let pos = app.controls[0].center();
        plot_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
        );
        plot_frame(&mut app, &ctx, vec![pointer(pos, false)]);
        assert_eq!(app.session.document.current_page().objects.len(), 2);
        assert!(
            matches!(&app.session.document.current_page().objects[1].kind, ObjectKind::FunctionPlot { expressions, .. } if expressions == &[board_math::plot_expression(input).unwrap()])
        );
        app.undo(false);
        assert_eq!(app.session.document.current_page().objects.len(), 1);
    }
}

#[test]
fn implicit_candidate_pointer_plot_single_undo_erase_and_regenerate() {
    for input in ["y²=x", "y-x=1", "x²+y²=4", "(x-1)^2/4+(y+2)^2/9=1"] {
        let mut app = app();
        app.set_ink_math_mode(InkMathMode::Confirm);
        let now = Instant::now();
        write_sample(&mut app, now);
        let original = app.session.document.current_page().objects.clone();
        let ctx = egui::Context::default();
        app.poll_ink(&ctx, now + AutoCalculate::IDLE_DELAY, move |_| {
            Ok(candidate(input, 0.7))
        });
        drain_workers(&mut app);
        assert_eq!(app.ink_action().unwrap().1, "f");
        assert_eq!(app.session.document.current_page().objects, original);
        let id = app.active_candidate.clone().unwrap();
        let expected = board_math::plot_expression(input).unwrap();
        click_ink_icon(&mut app, &ctx);
        let result = app
            .session
            .document
            .current_page()
            .objects
            .last()
            .unwrap()
            .clone();
        assert!(
            matches!(&result.kind, ObjectKind::FunctionPlot { expressions, .. } if expressions == &[expected])
        );
        assert!(app.ink_cache.get(&id).unwrap().result_present);
        assert!(!app.math_worker.busy());
        app.undo(false);
        assert_eq!(app.session.document.current_page().objects, original);
        click_ink_icon(&mut app, &ctx);
        assert_eq!(
            app.session.document.current_page().objects.last(),
            Some(&result)
        );
        app.tool = Tool::Eraser;
        let point = board_render::object_bounds(&result).center();
        app.controls.clear();
        eraser_frame(&mut app, &ctx, vec![]);
        eraser_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(point), pointer(point, true)],
        );
        assert_eq!(
            app.session.document.current_page().objects.len(),
            original.len() + 1
        );
        eraser_frame(&mut app, &ctx, vec![pointer(point, false)]);
        assert_eq!(app.session.document.current_page().objects, original);
        app.tool = Tool::Pen;
        click_ink_icon(&mut app, &ctx);
        assert_eq!(
            app.session.document.current_page().objects.last(),
            Some(&result)
        );
        app.undo(false);
        assert_eq!(app.session.document.current_page().objects, original);
        app.undo(true);
        plot_frame(&mut app, &ctx, vec![]);
        assert!(app.ink_cache.get(&id).unwrap().result_present);
        assert!(app.controls.is_empty());
    }
}

#[test]
fn mock_latex_conics_reach_parser_classifier_sampler_and_pointer_plot() {
    for latex in [
        r"\[y^{2}=x\]",
        r"\[y-x=1\]",
        r"\[x^{2}+y^{2}=4\]",
        r"\[\frac{(x-1)^{2}}{4}+\frac{(y+2)^{2}}{9}=1\]",
    ] {
        let mut app = app();
        app.set_hwr_backend(HwrBackend::TexTeller);
        app.set_ink_math_mode(InkMathMode::Confirm);
        let now = Instant::now();
        let count = write_sample(&mut app, now);
        let ctx = egui::Context::default();
        app.poll_ink_result(&ctx, now + AutoCalculate::IDLE_DELAY, move |_| {
            mock_neural(latex, true, -0.01)
        });
        drain_workers(&mut app);
        let parsed = board_hwr::latex_to_expression(latex).unwrap();
        assert_eq!(app.expression, parsed);
        assert_eq!(app.ink_action().unwrap().1, "f");
        let board_math::PlotKind::Implicit(equation) =
            board_math::classify_plot(&parsed).unwrap()
        else {
            panic!("{parsed}")
        };
        let (left, right) = equation.split_once('=').unwrap();
        let left = board_math::parse(left).unwrap();
        let right = board_math::parse(right).unwrap();
        let samples = board_math::sample_plot(
            &equation,
            board_math::Bounds2D {
                x_min: -10.0,
                x_max: 10.0,
                y_min: -7.0,
                y_max: 7.0,
            },
            128,
        )
        .unwrap();
        assert!(!samples.segments.is_empty());
        for point in samples.segments.iter().flatten() {
            assert!(
                (left.eval_xy(point.x, point.y).unwrap()
                    - right.eval_xy(point.x, point.y).unwrap())
                .abs()
                    < 1e-7
            );
        }
        if latex.contains("frac") {
            assert_eq!(samples.segments.len(), 1);
            assert_eq!(samples.segments[0].first(), samples.segments[0].last());
        }
        assert_eq!(app.session.document.current_page().objects.len(), count);
        click_ink_icon(&mut app, &ctx);
        assert!(
            matches!(&app.session.document.current_page().objects.last().unwrap().kind, ObjectKind::FunctionPlot { expressions, .. } if expressions == &[equation])
        );
        app.undo(false);
        assert_eq!(app.session.document.current_page().objects.len(), count);
    }
}

#[test]
fn one_variable_equations_keep_calculator_pointer_action() {
    for input in ["2x+3=x-1", "x=2", "x²=2"] {
        let mut app = app();
        app.set_ink_math_mode(InkMathMode::Confirm);
        let now = Instant::now();
        let count = write_sample(&mut app, now);
        let ctx = egui::Context::default();
        app.poll_ink(&ctx, now + AutoCalculate::IDLE_DELAY, move |_| {
            Ok(candidate(input, 0.9))
        });
        drain_workers(&mut app);
        assert_eq!(app.ink_action().unwrap().1, "=");
        click_ink_icon(&mut app, &ctx);
        drain_workers(&mut app);
        let expected = board_math::calculate(input).unwrap();
        assert!(
            matches!(&app.session.document.current_page().objects.last().unwrap().kind, ObjectKind::Text { text, .. } if text == &expected)
        );
        app.undo(false);
        assert_eq!(app.session.document.current_page().objects.len(), count);
    }
}

#[test]
fn function_hint_precedes_confirmation_and_uses_corrected_candidate() {
    for input in ["y=x^2", "f(x)=x^2"] {
        let mut app = app();
        app.set_ink_math_mode(InkMathMode::Confirm);
        let now = Instant::now();
        let count = write_sample(&mut app, now);
        let ctx = egui::Context::default();
        app.poll_ink(&ctx, now + AutoCalculate::IDLE_DELAY, move |_| {
            Ok(candidate(input, 0.7))
        });
        drain_workers(&mut app);
        assert_eq!(app.session.document.current_page().objects.len(), count);
        assert_eq!(app.ink_function().unwrap().1, "x^2");
        assert_eq!(app.ink_action().unwrap().1, "f");
        for _ in 0..100 {
            app.poll_workers(&ctx);
            assert_eq!(app.session.document.current_page().objects.len(), count);
        }
        app.expression = "y=x^3".into();
        click_ink_icon(&mut app, &ctx);
        app.plot_ink_function();
        app.confirm_calculation(&ctx);
        assert_eq!(app.session.document.current_page().objects.len(), count + 1);
        assert!(
            matches!(&app.session.document.current_page().objects.last().unwrap().kind,
            ObjectKind::FunctionPlot { expressions, .. } if expressions == &["x^3"])
        );
        app.undo(false);
        assert_eq!(app.session.document.current_page().objects.len(), count);
    }
}

#[test]
fn exact_ink_layout_is_one_object_after_icon_or_panel_click() {
    use board_core::MathLayout::{Fraction, Radical, Row, Text};
    for icon in [true, false] {
        for (input, result) in [
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
            let mut app = app();
            app.set_ink_math_mode(InkMathMode::Confirm);
            let now = Instant::now();
            write_sample(&mut app, now);
            let original = app.session.document.current_page().objects.clone();
            let ctx = egui::Context::default();
            app.poll_ink(&ctx, now + AutoCalculate::IDLE_DELAY, move |_| {
                Ok(candidate(input, 0.99))
            });
            drain_workers(&mut app);
            assert_eq!(app.session.document.current_page().objects, original);
            if icon {
                click_ink_icon(&mut app, &ctx);
            } else {
                app.confirm_calculation(&ctx);
            }
            drain_workers(&mut app);
            let objects = &app.session.document.current_page().objects;
            assert_eq!(objects.len(), original.len() + 1);
            assert!(
                matches!(&objects.last().unwrap().kind, ObjectKind::Math { layout, .. }
                if *layout == Row(vec![Text("= ".into()), result]))
            );
            app.undo(false);
            assert_eq!(app.session.document.current_page().objects, original);
            app.undo(true);
            assert_eq!(
                app.session.document.current_page().objects.len(),
                original.len() + 1
            );
        }
    }
}

#[test]
fn math_selection_move_erase_and_stale_preview_preserve_single_object() {
    let mut app = app();
    let output = features::calculation_display("1/2+1/3", app.math_bounds, true).unwrap();
    app.add(output.into_kind(Point { x: 300.0, y: 200.0 }, Color::default()));
    let original = app.session.document.current_page().objects[0].clone();
    let center = board_render::object_bounds(&original).center();
    assert!(editing::hit_test(&original, center, 0.0));
    app.tool = Tool::Select;
    let ctx = egui::Context::default();
    eraser_frame(&mut app, &ctx, vec![]);
    eraser_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(center), pointer(center, true)],
    );
    let end = center + Vec2::new(100.0, 80.0);
    eraser_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
    eraser_frame(&mut app, &ctx, vec![pointer(end, false)]);
    let mut expected = original.clone();
    editing::translate(&mut expected, Point { x: 100.0, y: 80.0 });
    assert_eq!(
        app.session.document.current_page().objects,
        vec![expected.clone()]
    );
    app.undo(false);
    assert_eq!(app.session.document.current_page().objects, vec![original]);
    app.undo(true);
    app.tool = Tool::Eraser;
    eraser_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(end), pointer(end, true)],
    );
    assert!(erased_document(&app).current_page().objects.is_empty());
    assert_eq!(
        app.session.document.current_page().objects,
        vec![expected.clone()]
    );
    app.session.document.revision += 1;
    eraser_frame(&mut app, &ctx, vec![pointer(end, false)]);
    assert_eq!(
        app.session.document.current_page().objects,
        vec![expected.clone()]
    );
    app.session.history = board_core::History::new(&app.session.document);
    eraser_frame(&mut app, &ctx, vec![]);
    eraser_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(end), pointer(end, true)],
    );
    let erase_end = end + Vec2::new(20.0, 0.0);
    eraser_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(erase_end)]);
    assert!(erased_document(&app).current_page().objects.is_empty());
    eraser_frame(&mut app, &ctx, vec![pointer(erase_end, false)]);
    assert!(
        app.session.document.current_page().objects.is_empty(),
        "{}",
        app.status
    );
    app.undo(false);
    assert_eq!(app.session.document.current_page().objects, vec![expected]);
    app.undo(true);
    assert!(app.session.document.current_page().objects.is_empty());
}
