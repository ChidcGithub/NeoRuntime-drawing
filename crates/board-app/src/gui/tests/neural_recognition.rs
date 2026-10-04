#[test]
fn simulated_texteller_answer_prompt_writes_once_to_right_and_undo_preserves_ink() {
    // Inject the reported model output; this is not an actual model acceptance test.
    for (latex, expected) in [(r"\[1+1=\]", "2"), (r"\[1+1\]", "= 2")] {
        let mut app = app();
        app.set_hwr_backend(HwrBackend::TexTeller);
        app.set_ink_math_mode(InkMathMode::Confirm);
        let now = Instant::now();
        write_sample(&mut app, now);
        if expected == "2" {
            for y in [94.0, 106.0] {
                app.ink_started();
                app.gesture = Some(Gesture {
                    document: app.session.document.id.clone(),
                    page: app.session.document.current_page().id.clone(),
                    revision: app.session.document.revision,
                    points: [210.0, 240.0]
                        .into_iter()
                        .map(|x| StrokePoint {
                            x,
                            y,
                            time: 0.0,
                            pressure: 0.6,
                        })
                        .collect(),
                    original: None,
                    vertex: None,
                    resize: None,
                    erasing: None,
                });
                app.finish_gesture_at(now);
            }
        }
        let original = app.session.document.current_page().objects.clone();
        let bounds = features::ink_bounds(&app.ink);
        let ctx = egui::Context::default();
        app.poll_ink_result(&ctx, now + AutoCalculate::IDLE_DELAY, move |_| {
            mock_neural(latex, true, -0.02111)
        });
        drain_workers(&mut app);
        assert_eq!(app.expression, "(1+1)");
        let raw = app.neural_raw.as_ref().unwrap();
        assert_eq!(raw.latex, latex);
        assert_eq!(raw.generated_tokens, 6);
        assert_eq!(raw.mean_log_probability, -0.02111);
        assert!(raw.finished);
        assert_eq!(app.session.document.current_page().objects, original);
        click_ink_icon(&mut app, &ctx);
        drain_workers(&mut app);
        assert_eq!(app.math_result, expected);
        let objects = &app.session.document.current_page().objects;
        assert_eq!(objects.len(), original.len() + 1);
        assert_eq!(&objects[..original.len()], original.as_slice());
        assert!(matches!(&objects.last().unwrap().kind,
            ObjectKind::Math { layout, position, .. }
                if features::layout_text(layout) == expected && *position == features::result_position(bounds, false)
                    && position.x > bounds.right()));
        app.poll_ink_result(&ctx, now + Duration::from_secs(5), |_| panic!("duplicate"));
        drain_workers(&mut app);
        assert_eq!(
            app.session.document.current_page().objects.len(),
            original.len() + 1
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

#[test]
fn simulated_texteller_radical_fraction_reaches_exact_math_and_single_undo() {
    use board_core::MathLayout;
    fn nodes(layout: &MathLayout) -> (usize, usize) {
        match layout {
            MathLayout::Text(text) => {
                assert!(!text.contains('='));
                (0, 0)
            }
            MathLayout::Row(children) => children
                .iter()
                .map(nodes)
                .fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1)),
            MathLayout::Fraction(numerator, denominator) => {
                let a = nodes(numerator);
                let b = nodes(denominator);
                (1 + a.0 + b.0, a.1 + b.1)
            }
            MathLayout::Radical(child) => {
                let (fractions, radicals) = nodes(child);
                (fractions, radicals + 1)
            }
        }
    }
    let latex = r"\[\frac{3+2 \sqrt{3}}{2}=\]";
    let mut app = app();
    app.set_hwr_backend(HwrBackend::TexTeller);
    app.set_ink_math_mode(InkMathMode::Confirm);
    let now = Instant::now();
    write_sample(&mut app, now);
    let original = app.session.document.current_page().objects.clone();
    let revision = app.session.document.revision;
    let bounds = features::ink_bounds(&app.ink);
    let ctx = egui::Context::default();
    app.poll_ink_result(&ctx, now + AutoCalculate::IDLE_DELAY, move |_| {
        neural_result(NeuralFormula {
            latex: latex.into(),
            finished: true,
            generated_tokens: 14,
            mean_log_probability: -0.000267,
        })
    });
    drain_workers(&mut app);
    assert_eq!(app.neural_raw.as_ref().unwrap().latex, latex);
    assert_eq!(app.expression, "((3+(2*sqrt(3)))/2)");
    assert_eq!(app.session.document.current_page().objects, original);
    click_ink_icon(&mut app, &ctx);
    drain_workers(&mut app);
    let exact = board_math::calculate_display_with_bounds(
        "3/2+sqrt(3)",
        app.math_bounds,
        Default::default(),
    )
    .unwrap();
    assert!(!exact.approximate);
    assert_eq!(exact.text, "(3 + 2*sqrt(3))/2");
    assert_eq!(app.math_result, exact.text);
    assert!(!app.math_result.contains('='));
    assert_eq!(
        board_math::calculate(&format!("({})-(3/2+sqrt(3))", app.math_result)).unwrap(),
        "0"
    );
    let objects = &app.session.document.current_page().objects;
    assert_eq!(objects.len(), original.len() + 1);
    assert_eq!(&objects[..original.len()], original.as_slice());
    let ObjectKind::Math {
        layout, position, ..
    } = &objects.last().unwrap().kind
    else {
        panic!("精确结果应为二维 Math 对象");
    };
    let (fractions, radicals) = nodes(layout);
    assert!(fractions > 0 && radicals > 0);
    assert_eq!(*position, features::result_position(bounds, false));
    assert_eq!(app.session.document.revision, revision + 1);
    app.poll_ink_result(&ctx, now + Duration::from_secs(5), |_| panic!("duplicate"));
    drain_workers(&mut app);
    assert_eq!(
        app.session.document.current_page().objects.len(),
        original.len() + 1
    );
    app.undo(false);
    assert_eq!(app.session.document.current_page().objects, original);
    app.undo(true);
    assert_eq!(
        app.session.document.current_page().objects.len(),
        original.len() + 1
    );
}

#[test]
fn malformed_texteller_brace_reports_reason_and_never_writes_on_repeated_delivery() {
    let latex = r"\[\frac{3+2 \sqrt{3}}}{2}=\]";
    let result = move || {
        neural_result(NeuralFormula {
            latex: latex.into(),
            finished: true,
            generated_tokens: 14,
            mean_log_probability: -0.000267,
        })
    };
    let mut app = app();
    app.set_hwr_backend(HwrBackend::TexTeller);
    app.set_ink_math_mode(InkMathMode::Confirm);
    let now = Instant::now();
    write_sample(&mut app, now);
    let original = app.session.document.current_page().objects.clone();
    let revision = app.session.document.revision;
    let ctx = egui::Context::default();
    app.poll_ink_result(&ctx, now + AutoCalculate::IDLE_DELAY, move |_| result());
    let request = app.calculation.clone().unwrap();
    drain_workers(&mut app);
    for _ in 0..3 {
        let raw = app.neural_raw.as_ref().unwrap();
        assert_eq!(raw.latex, latex);
        assert!(raw.mean_log_probability > -0.001);
        assert_eq!(raw.generated_tokens, 14);
        let error = app
            .ink_diagnostic
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .unwrap();
        assert!(error.contains("LaTeX 右花括号多余/不匹配，位置：第 11 个词元"));
        assert!(error.contains("表达式输入框人工纠错"));
        assert!(app.math_result.contains(error));
        assert!(app.recognition.is_none());
        assert!(app.expression.is_empty());
        assert_eq!(app.ink_action().unwrap().1, "?");
        assert_eq!(app.session.document.current_page().objects, original);
        assert_eq!(app.session.document.revision, revision);
        // 模拟相同请求再次投递（即使仍有有效 ticket），不能绕过解析错误。
        app.ink_ticket = Some(request.id().to_owned());
        let work = request.clone();
        app.hwr_worker.start(ctx.clone(), move || (work, result()));
        drain_workers(&mut app);
    }
    assert_eq!(app.session.document.current_page().objects, original);
    assert_eq!(app.session.document.revision, revision);
}

#[test]
fn simulated_prompt_metadata_does_not_follow_manual_correction() {
    let mut app = app();
    app.set_hwr_backend(HwrBackend::TexTeller);
    app.set_ink_math_mode(InkMathMode::Confirm);
    let now = Instant::now();
    write_sample(&mut app, now);
    let ctx = egui::Context::default();
    app.poll_ink_result(&ctx, now + AutoCalculate::IDLE_DELAY, |_| {
        mock_neural(r"\[1+1=\]", true, -0.02111)
    });
    drain_workers(&mut app);
    app.expression = "1+2".into();
    app.confirm_calculation(&ctx);
    drain_workers(&mut app);
    assert_eq!(app.math_result, "= 3");
}

#[test]
fn neural_scores_and_completion_never_trigger_output_on_many_polls() {
    let ctx = egui::Context::default();
    for (finished, lp, latex) in [
        (true, -0.01, r"\[1+1\]"),
        (false, -0.01, r"\[1+1\]"),
        (true, -0.3, r"\[1+1\]"),
        (true, f64::NAN, r"\[1+1\]"),
        (true, f64::INFINITY, r"\[1+1\]"),
        (true, 0.01, r"\[1+1\]"),
        (true, -0.01, r"\[1+\unknown{1}\]"),
        (true, -0.01, r"\[1/0\]"),
        (true, -0.01, r"\[y=x^2\]"),
        (true, -0.02111, r"\[1+1=\]"),
        (true, -0.02111, r"\[x+1=\]"),
        (true, -0.02111, r"\[y=\]"),
        (true, -0.02111, r"\[f(x)=\]"),
        (true, -0.02111, r"\[(1+1=)\]"),
        (true, -0.02111, r"\[1+1==\]"),
        (true, -0.02111, r"\[x=1;y=\]"),
        (true, -0.02111, r"\[1/0=\]"),
    ] {
        let mut app = app();
        app.set_hwr_backend(HwrBackend::TexTeller);
        app.set_ink_math_mode(InkMathMode::Confirm);
        let now = Instant::now();
        let count = write_sample(&mut app, now);
        app.poll_ink_result(&ctx, now + AutoCalculate::IDLE_DELAY, move |_| {
            mock_neural(latex, finished, lp)
        });
        drain_workers(&mut app);
        for _ in 0..100 {
            app.poll_workers(&ctx);
            app.poll_ink_result(&ctx, now + Duration::from_secs(20), |_| panic!("duplicate"));
            assert!(!app.math_worker.busy());
            assert_eq!(
                app.session.document.current_page().objects.len(),
                count,
                "{latex} {lp}"
            );
        }
        assert!(!app.math);
        assert_eq!(app.neural_raw.as_ref().unwrap().latex, latex);
        if let Some(recognition) = &app.recognition {
            assert_eq!(recognition.confidence, 0.0);
            assert!(recognition.requires_confirmation);
        } else {
            assert_eq!(app.ink_action().unwrap().1, "?");
        }
        app.expression = "1+2".into();
        app.confirm_calculation(&ctx);
        drain_workers(&mut app);
        assert_eq!(app.math_result, "= 3");
    }
}

#[test]
fn backend_switch_reload_and_cancel_discard_stale_neural_work() {
    for change in 0..3 {
        let mut app = app();
        app.set_hwr_backend(HwrBackend::TexTeller);
        app.set_ink_math_mode(InkMathMode::Confirm);
        let now = Instant::now();
        let count = write_sample(&mut app, now);
        let (tx, rx) = mpsc::channel();
        let ctx = egui::Context::default();
        app.poll_ink_result(&ctx, now + AutoCalculate::IDLE_DELAY, move |_| {
            rx.recv().unwrap();
            mock_neural(r"\[1+1\]", true, -0.01)
        });
        match change {
            0 => {
                app.set_hwr_backend(HwrBackend::Template);
                app.set_hwr_backend(HwrBackend::TexTeller);
            }
            1 => {
                app.model_dir = "relative-invalid-path".into();
                app.load_model(&ctx);
            }
            _ => {
                app.cancel_ink();
            }
        }
        assert!(app.hwr_worker.busy());
        tx.send(()).unwrap();
        drain_workers(&mut app);
        assert!(app.recognition.is_none());
        assert!(app.neural_raw.is_none());
        assert!(app.ink_diagnostic.is_none());
        assert!(app.calculation.is_none());
        assert_eq!(app.session.document.current_page().objects.len(), count);
    }
}

#[test]
fn model_load_slot_never_queues_or_blocks_and_template_discards_failure() {
    let mut app = app();
    app.set_hwr_backend(HwrBackend::TexTeller);
    let ctx = egui::Context::default();
    let (tx, rx) = mpsc::channel();
    app.model_loader.start(ctx.clone(), move || {
        rx.recv().unwrap();
        Err("mock model load failure".into())
    });
    app.load_model(&ctx);
    assert!(app.model_loader.busy());
    app.set_hwr_backend(HwrBackend::Template);
    app.poll_workers(&ctx);
    assert!(app.model_loader.busy());
    assert!(app.neural_recognizer.is_none());
    tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while app.model_loader.busy() {
        app.poll_workers(&ctx);
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(!app.model_status.contains("mock model load failure"));
    assert!(app.loaded_model_dir.is_empty());
    assert!(app.neural_recognizer.is_none());
}

#[test]
fn neural_gate_accepts_full_budget_without_truncating_snapshot() {
    for (backend, strokes, points, accepted) in [
        (HwrBackend::Template, 33, 2, false),
        (HwrBackend::TexTeller, 128, 256, true),
    ] {
        let mut app = app();
        app.set_hwr_backend(backend);
        app.set_ink_math_mode(InkMathMode::Confirm);
        let now = Instant::now();
        let input = vec![
            vec![
                StrokePoint {
                    x: 1.0,
                    y: 1.0,
                    time: 0.0,
                    pressure: 0.5
                };
                points
            ];
            strokes
        ];
        let context = ContextToken::capture(&app.session.document);
        app.auto_gate
            .strokes_finished(now, context, &input)
            .unwrap();
        app.poll_ink_result(
            &egui::Context::default(),
            now + AutoCalculate::IDLE_DELAY,
            move |ink| {
                assert!(accepted);
                assert_eq!(ink.len(), strokes);
                assert_eq!(ink.iter().map(Vec::len).sum::<usize>(), strokes * points);
                mock_neural(r"\(1+1\)", true, -0.01)
            },
        );
        drain_workers(&mut app);
        assert_eq!(app.recognition.is_some(), accepted);
    }
}

#[test]
#[ignore = "真实 TexTeller ONNX CPU 集成；需显式设置 TEXTELLER_MODEL_DIR"]
fn actual_texteller_gui_click_write_and_single_undo() {
    let mut app = app();
    app.set_hwr_backend(HwrBackend::TexTeller);
    app.set_ink_math_mode(InkMathMode::Confirm);
    app.model_dir = std::env::var("TEXTELLER_MODEL_DIR").expect("设置绝对模型目录");
    let ctx = egui::Context::default();
    app.load_model(&ctx);
    assert!(app.model_loader.busy());
    let deadline = Instant::now() + Duration::from_secs(180);
    while app.model_loader.busy() {
        app.poll_workers(&ctx);
        assert!(Instant::now() < deadline, "加载超时");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(app.neural_recognizer.is_some(), "{}", app.model_status);
    let now = Instant::now();
    for points in [
        vec![(15., 25.), (25., 15.), (25., 85.)],
        vec![(58., 50.), (98., 50.)],
        vec![(78., 30.), (78., 70.)],
        vec![(122., 25.), (132., 15.), (132., 85.)],
    ] {
        app.ink_started();
        app.gesture = Some(Gesture {
            document: app.session.document.id.clone(),
            page: app.session.document.current_page().id.clone(),
            revision: app.session.document.revision,
            points: points
                .into_iter()
                .map(|(x, y)| StrokePoint {
                    x,
                    y,
                    time: 0.0,
                    pressure: 0.5,
                })
                .collect(),
            original: None,
            vertex: None,
            resize: None,
            erasing: None,
        });
        app.finish_gesture_at(now);
    }
    let count = app.session.document.current_page().objects.len();
    assert_eq!(count, 4);
    assert_eq!(app.ink.len(), 4);
    app.poll_backend_ink(&ctx, now + Duration::from_millis(2499));
    assert!(!app.hwr_worker.busy());
    app.poll_backend_ink(&ctx, now + AutoCalculate::IDLE_DELAY);
    assert!(app.hwr_worker.busy());
    while app.hwr_worker.busy() || app.math_worker.busy() {
        app.poll_workers(&ctx);
        assert!(Instant::now() < deadline, "推理超时");
        std::thread::sleep(Duration::from_millis(10));
    }
    let raw = app.neural_raw.as_ref().expect("必须真实产生原始输出");
    println!(
        "真实 GUI strokes->TexTeller: {raw:?}; math={}",
        app.math_result
    );
    assert!(raw.finished);
    assert_eq!(
        board_math::calculate(&latex_to_expression(&raw.latex).unwrap()).unwrap(),
        "2"
    );
    assert_eq!(app.session.document.current_page().objects.len(), count);
    assert!(!app.math_worker.busy());
    click_ink_icon(&mut app, &ctx);
    drain_workers(&mut app);
    assert_eq!(
        app.session.document.current_page().objects.len(),
        count + 1,
        "{}",
        app.math_result
    );
    assert!(
        matches!(&app.session.document.current_page().objects.last().unwrap().kind, ObjectKind::Math { layout, .. } if features::layout_text(layout) == "= 2")
    );
    app.poll_backend_ink(&ctx, now + Duration::from_secs(10));
    assert!(!app.hwr_worker.busy());
    app.undo(false);
    assert_eq!(app.session.document.current_page().objects.len(), count);
    app.undo(true);
    assert_eq!(app.session.document.current_page().objects.len(), count + 1);
}
