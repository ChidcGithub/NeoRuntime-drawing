fn panel_frame(
    app: &mut BoardApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1600.0, 1600.0))),
            events,
            ..Default::default()
        },
        |_| app.panels(ctx),
    );
    output.textures_delta.clear();
    output
}

fn click_panel_text(app: &mut BoardApp, ctx: &egui::Context, text: &str) -> egui::FullOutput {
    panel_frame(app, ctx, vec![]);
    let output = panel_frame(app, ctx, vec![]);
    let pos = text_position(&output, text);
    assert!(ctx.content_rect().contains(pos));
    panel_frame(
        app,
        ctx,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
    );
    panel_frame(app, ctx, vec![pointer(pos, false)]);
    // Let the window and scroll area resize to the newly expanded contents.
    for _ in 0..3 {
        panel_frame(app, ctx, vec![egui::Event::PointerGone]);
    }
    panel_frame(app, ctx, vec![])
}

#[test]
fn recognition_diagnostics_start_closed_and_pointer_click_reveals_input_and_raw_latex() {
    fn has_texture(shape: &egui::Shape, texture: egui::TextureId) -> bool {
        match shape {
            egui::Shape::Mesh(mesh) => mesh.texture_id == texture,
            egui::Shape::Rect(rect) => rect
                .brush
                .as_ref()
                .is_some_and(|brush| brush.fill_texture_id == texture),
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| has_texture(shape, texture)),
            _ => false,
        }
    }

    let mut app = app();
    app.set_hwr_backend(HwrBackend::TexTeller);
    app.set_ink_math_mode(InkMathMode::Confirm);
    let now = Instant::now();
    write_sample(&mut app, now);
    let original = app.session.document.clone();
    let ctx = egui::Context::default();
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
    }
    let latex = r"\[1+\unknown{1}\]";
    app.poll_ink_result(&ctx, now + AutoCalculate::IDLE_DELAY, |_| {
        mock_neural(latex, true, -0.00001)
    });
    drain_workers(&mut app);
    click_ink_icon(&mut app, &ctx);
    assert!(app.math);
    let diagnostic = app.ink_diagnostic.as_ref().unwrap();
    let texture = diagnostic.texture.as_ref().unwrap().id();
    let request_label = format!("请求：{}", diagnostic.request_id);
    let snapshot_label = format!(
        "快照：{} 笔 / {} 点；逐笔点数（书写顺序）：{:?}",
        diagnostic.stroke_points.len(),
        diagnostic.stroke_points.iter().sum::<usize>(),
        diagnostic.stroke_points,
    );
    let error = diagnostic.error.clone().unwrap();
    let raw_header = "查看原始 LaTeX / 模型分数（非准确概率）";
    panel_frame(&mut app, &ctx, vec![]);
    let closed = panel_frame(&mut app, &ctx, vec![]);
    text_position(&closed, "本次识别输入");
    text_position(&closed, raw_header);
    text_position(&closed, &error);
    text_position(&closed, "模型可能看错，请核对后点击计算/绘图。");
    let text = format!("{:?}", closed.shapes);
    assert!(!text.contains("快照："));
    assert!(!text.contains("原始 LaTeX："));
    assert!(!text.contains("mean logprob="));
    assert!(!closed.shapes.iter().any(|s| has_texture(&s.shape, texture)));

    let preview = click_panel_text(&mut app, &ctx, "本次识别输入");
    text_position(&preview, &request_label);

    let pos = text_position(&preview, &request_label);
    for delta in [-200.0, -350.0, 1000.0] {
        panel_frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, delta),
                    phase: egui::TouchPhase::Move,
                    modifiers: Default::default(),
                },
            ],
        );
        let mut scrolled = panel_frame(&mut app, &ctx, vec![]);
        for _ in 0..30 {
            scrolled = panel_frame(&mut app, &ctx, vec![]);
        }
        if delta == -200.0 {
            text_position(&scrolled, &snapshot_label);
            assert!(
                scrolled
                    .shapes
                    .iter()
                    .any(|s| has_texture(&s.shape, texture))
            );
        } else if delta == -350.0 {
            text_position(&scrolled, "448×448 模型输入预览");
        }
    }
    click_panel_text(&mut app, &ctx, "本次识别输入");
    let raw = click_panel_text(&mut app, &ctx, raw_header);
    text_position(&raw, latex);
    text_position(&raw, "finished=true，tokens=6，mean logprob=-0.000010");
    text_position(&raw, &error);
    assert!(!app.math_worker.busy());
    assert_eq!(app.session.document, original);
}

#[test]
fn malformed_latex_icon_only_opens_correction_and_keeps_preview() {
    let mut app = app();
    app.set_hwr_backend(HwrBackend::TexTeller);
    app.set_ink_math_mode(InkMathMode::Confirm);
    let now = Instant::now();
    let count = write_sample(&mut app, now);
    let ctx = egui::Context::default();
    app.poll_ink_result(&ctx, now + AutoCalculate::IDLE_DELAY, |_| {
        mock_neural(r"\[1+\unknown{1}\]", true, -0.00001)
    });
    drain_workers(&mut app);
    assert!(!app.math);
    assert_eq!(app.ink_action().unwrap().1, "?");
    click_ink_icon(&mut app, &ctx);
    assert!(app.math);
    assert!(app.expression.is_empty());
    assert!(app.ink_diagnostic.as_ref().unwrap().texture.is_some());
    assert_eq!(app.neural_raw.as_ref().unwrap().latex, r"\[1+\unknown{1}\]");
    assert!(!app.math_worker.busy());
    assert_eq!(app.session.document.current_page().objects.len(), count);
}

#[test]
fn busy_ink_icon_does_not_consume_or_draw_a_stroke() {
    for (loading, input) in [
        (false, "1+1"),
        (true, "1+1"),
        (false, "y²=x"),
        (true, "y²=x"),
    ] {
        let mut app = app();
        app.set_ink_math_mode(InkMathMode::Confirm);
        let now = Instant::now();
        let count = write_sample(&mut app, now);
        let ctx = egui::Context::default();
        app.poll_ink(&ctx, now + AutoCalculate::IDLE_DELAY, move |_| {
            Ok(candidate(input, 1.0))
        });
        drain_workers(&mut app);
        let (tx, rx) = mpsc::channel();
        if loading {
            app.model_loader.start(ctx.clone(), move || {
                rx.recv().unwrap();
                Err("test load failure".into())
            });
        } else {
            app.start_math(&ctx, move || {
                rx.recv().unwrap();
                Ok(("test".into(), None))
            });
        }
        click_ink_icon(&mut app, &ctx);
        assert!(app.calculation.is_some());
        assert_eq!(app.session.document.current_page().objects.len(), count);
        tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while app.ink_action_busy() {
            app.poll_workers(&egui::Context::default());
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        click_ink_icon(&mut app, &ctx);
        drain_workers(&mut app);
        if input == "1+1" {
            assert_eq!(app.math_result, "= 2");
        } else {
            assert!(
                matches!(&app.session.document.current_page().objects.last().unwrap().kind, ObjectKind::FunctionPlot { expressions, .. } if expressions == &["y^2=x"])
            );
        }
        assert_eq!(app.session.document.current_page().objects.len(), count + 1);
    }
}

#[test]
fn candidate_icon_clears_on_cancel_pen_page_undo_clear_close_backend_and_reload() {
    for change in 0..8 {
        let mut app = app();
        app.set_ink_math_mode(InkMathMode::Confirm);
        let now = Instant::now();
        write_sample(&mut app, now);
        let ctx = egui::Context::default();
        app.poll_ink(&ctx, now + AutoCalculate::IDLE_DELAY, |_| {
            Ok(candidate("1+1", 1.0))
        });
        drain_workers(&mut app);
        assert!(app.ink_action().is_some());
        match change {
            0 => app.cancel_ink(),
            1 => app.ink_started(),
            2 => {
                app.session.document.add_page().unwrap();
            }
            3 => app.undo(false),
            4 => {
                let ops = app
                    .session
                    .document
                    .current_page()
                    .objects
                    .iter()
                    .map(|object| Operation::Delete {
                        id: object.id.clone(),
                    })
                    .collect();
                app.apply(ops);
            }
            5 => app.request_close(&ctx),
            6 => app.set_hwr_backend(HwrBackend::TexTeller),
            _ => {
                app.hwr_backend = HwrBackend::TexTeller;
                app.model_dir = "relative-invalid-path".into();
                app.load_model(&ctx);
            }
        }
        app.poll_workers(&ctx);
        assert!(app.ink_action().is_none());
        assert!(app.calculation.is_none());
        assert!(app.recognition.is_none());
        let count = app.session.document.current_page().objects.len();
        app.confirm_calculation(&ctx);
        drain_workers(&mut app);
        assert_eq!(app.session.document.current_page().objects.len(), count);
    }
}

#[test]
fn neural_preview_survives_failure_but_not_input_context_backend_or_cancel() {
    for change in 0..4 {
        for inference_error in [false, true] {
            let mut app = app();
            app.set_hwr_backend(HwrBackend::TexTeller);
            app.set_ink_math_mode(InkMathMode::Confirm);
            let now = Instant::now();
            let count = write_sample(&mut app, now);
            let expected_points: Vec<_> = app.ink.iter().map(Vec::len).collect();
            let ctx = egui::Context::default();
            app.poll_ink_result(&ctx, now + AutoCalculate::IDLE_DELAY, move |_| {
                let mut result = mock_neural(r"\[\bullet\]", true, -0.01);
                if inference_error {
                    result.neural = None;
                    result.recognition = Err("本地模型推理未完成".into());
                }
                result
            });
            let request = app.calculation.clone().unwrap();
            drain_workers(&mut app);
            let diagnostic = app.ink_diagnostic.as_ref().unwrap();
            assert_eq!(diagnostic.request_id, request.id());
            assert_eq!(diagnostic.context, request.context);
            assert_eq!(diagnostic.stroke_points, expected_points);
            assert_eq!(diagnostic.texture.as_ref().unwrap().size(), [448, 448]);
            assert!(diagnostic.error.is_some());
            if !inference_error {
                assert_eq!(app.neural_raw.as_ref().unwrap().latex, r"\[\bullet\]");
                let error = diagnostic.error.as_deref().unwrap();
                assert!(error.contains(r"unsupported: 命令 \bullet"));
                assert!(error.contains("表达式输入框人工纠错"));
            }
            assert!(app.recognition.is_none());
            assert_eq!(app.session.document.current_page().objects.len(), count);
            match change {
                0 => app.ink_started(),
                1 => app.undo(false),
                2 => app.set_hwr_backend(HwrBackend::Template),
                _ => app.cancel_ink(),
            }
            assert!(app.ink_diagnostic.is_none());
            assert!(app.neural_raw.is_none());
        }
    }
}

#[test]
fn preview_statistics_follow_grouping_without_changing_strokes() {
    let mut app = app();
    app.set_hwr_backend(HwrBackend::TexTeller);
    app.set_ink_math_mode(InkMathMode::Confirm);
    let now = Instant::now();
    write_sample(&mut app, now);
    let excluded = (app.ink.len(), app.ink.iter().map(Vec::len).sum::<usize>());
    let original = app.session.document.current_page().objects.clone();
    for x in [700.0, 730.0] {
        app.ink_started();
        app.gesture = Some(Gesture {
            document: app.session.document.id.clone(),
            page: app.session.document.current_page().id.clone(),
            revision: app.session.document.revision,
            points: [700.0, 730.0]
                .into_iter()
                .map(|y| StrokePoint {
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
    assert_eq!(app.ink.len(), 2);
    assert_eq!(app.ink_excluded, excluded);
    let expected_points: Vec<_> = app.ink.iter().map(Vec::len).collect();
    let ctx = egui::Context::default();
    app.poll_ink_result(&ctx, now + AutoCalculate::IDLE_DELAY, move |strokes| {
        assert_eq!(strokes.len(), 2);
        mock_neural(r"\[\bullet\]", true, -0.01)
    });
    drain_workers(&mut app);
    let diagnostic = app.ink_diagnostic.as_ref().unwrap();
    assert_eq!(diagnostic.stroke_points, expected_points);
    assert_eq!(diagnostic.excluded, excluded);
    assert_eq!(
        &app.session.document.current_page().objects[..original.len()],
        original.as_slice()
    );
}
