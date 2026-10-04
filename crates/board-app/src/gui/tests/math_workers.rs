#[test]
fn compact_math_help_keeps_limits_visible_and_template_overwrite_warning_reachable() {
    let mut app = app();
    app.math = true;
    let original = app.session.document.clone();
    let ctx = egui::Context::default();
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
    }
    panel_frame(&mut app, &ctx, vec![]);
    let closed = panel_frame(&mut app, &ctx, vec![]);
    for label in [
        "识别结果须核对/纠错后点击计算；分数不是正确概率。",
        "二元二次求解仅给范围内数值候选，不保证全解。",
        "数值微分/积分仅为近似，不支持广义积分。",
        "数学语法与限制",
        "学习个人符号模板（只在本机）",
        "计算",
        "化简",
        "解方程",
        "将手动输入计算后写到板上",
    ] {
        text_position(&closed, label);
    }
    let text = format!("{:?}", closed.shapes);
    assert!(!text.contains("512网格"));
    assert!(!text.contains("simplify(x+1=2)"));
    let help = click_panel_text(&mut app, &ctx, "数学语法与限制");
    text_position(
        &help,
        "符号运算仅支持有限 x/y 多项式；原函数积分常数取 0，不表示完整通解。",
    );
    assert!(format!("{:?}", help.shapes).contains("simplify(x+1=2)"));
    let templates = click_panel_text(&mut app, &ctx, "学习个人符号模板（只在本机）");
    text_position(&templates, "模板 JSON 路径（保存会覆盖该文件）");
    text_position(&templates, "保存模板到指定路径");
    text_position(&templates, "加载指定模板");
    assert!(app.ink_math_mode == InkMathMode::Off);
    assert!(!app.authorize_write_back);
    assert!(!app.math_worker.busy());
    assert_eq!(app.session.document, original);
}

#[test]
fn clicked_equation_newline_and_duplicate_worker_are_one_shot() {
    let mut app = app();
    app.set_ink_math_mode(InkMathMode::Confirm);
    let now = Instant::now();
    let count = write_sample(&mut app, now);
    let bounds = features::ink_bounds(&app.ink);
    let ctx = egui::Context::default();
    app.poll_ink(&ctx, now + AutoCalculate::IDLE_DELAY, |_| {
        Ok(candidate("2x+3=x-1", 0.99))
    });
    let request = app.calculation.clone().unwrap();
    drain_workers(&mut app);
    assert_eq!(app.session.document.current_page().objects.len(), count);
    click_ink_icon(&mut app, &ctx);
    drain_workers(&mut app);
    assert_eq!(app.session.document.current_page().objects.len(), count + 1);
    assert!(
        matches!(&app.session.document.current_page().objects.last().unwrap().kind,
        ObjectKind::Text { text, position, .. } if text == "x = -4" && *position == features::result_position(bounds, true))
    );
    app.hwr_worker.start(ctx.clone(), move || {
        (request, Ok(candidate("2x+3=x-1", 0.99)).into())
    });
    drain_workers(&mut app);
    assert_eq!(app.session.document.current_page().objects.len(), count + 1);
}

#[test]
fn stale_recognition_is_discarded_for_all_context_changes() {
    for change in 0..6 {
        let mut app = app();
        app.set_ink_math_mode(InkMathMode::Confirm);
        let now = Instant::now();
        write_sample(&mut app, now);
        let ctx = egui::Context::default();
        let (tx, rx) = mpsc::channel();
        app.poll_ink(&ctx, now + AutoCalculate::IDLE_DELAY, move |_| {
            rx.recv().unwrap();
            Ok(candidate("1+1", 0.99))
        });
        match change {
            0 => app.set_ink_math_mode(InkMathMode::Off),
            1 => app.ink_started(),
            2 => app.session.document.revision += 1,
            3 => {
                app.session.document.add_page().unwrap();
            }
            4 => app.session.document.id = new_id(),
            _ => {
                app.set_ink_math_mode(InkMathMode::Off);
                app.set_ink_math_mode(InkMathMode::Confirm);
            }
        }
        let count = app.session.document.current_page().objects.len();
        tx.send(()).unwrap();
        drain_workers(&mut app);
        assert_eq!(
            app.session.document.current_page().objects.len(),
            count,
            "change {change}"
        );
        assert!(app.recognition.is_none());
        assert!(app.ink_action().is_none());
    }
}

#[test]
fn clicked_math_delivery_is_invalidated_after_recognition() {
    for change in 0..9 {
        let mut app = app();
        app.set_ink_math_mode(InkMathMode::Confirm);
        let now = Instant::now();
        write_sample(&mut app, now);
        let ctx = egui::Context::default();
        app.poll_ink(&ctx, now + AutoCalculate::IDLE_DELAY, |_| {
            Ok(candidate("1+1", 0.99))
        });
        drain_workers(&mut app);
        assert!(!app.math_worker.busy());
        app.confirm_calculation(&ctx);
        assert!(app.math_worker.busy());
        match change {
            0 => app.set_ink_math_mode(InkMathMode::Off),
            1 => app.ink_started(),
            2 => app.session.document.revision += 1,
            3 => {
                app.session.document.add_page().unwrap();
            }
            4 => app.session.document.id = new_id(),
            5 => app.expression = "2+2".into(),
            6 => app.cancel_ink(),
            7 => app.set_hwr_backend(HwrBackend::TexTeller),
            _ => {
                app.hwr_backend = HwrBackend::TexTeller;
                app.model_dir = "relative-invalid-path".into();
                app.load_model(&ctx);
            }
        }
        let count = app.session.document.current_page().objects.len();
        drain_workers(&mut app);
        assert_eq!(
            app.session.document.current_page().objects.len(),
            count,
            "change {change}"
        );
        assert!(app.math_ticket.is_none());
    }
}

fn drain_workers(app: &mut BoardApp) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while app.math_worker.busy() || app.hwr_worker.busy() || app.plot_worker.busy() {
        app.poll_workers(&egui::Context::default());
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}

fn text_kind(text: &str) -> ObjectKind {
    ObjectKind::Text {
        position: Point::default(),
        text: text.into(),
        size: 24.0,
        color: Color::default(),
    }
}

fn arm_confirmation(app: &mut BoardApp, expression: &str) -> CalculationRequest {
    app.set_ink_math_mode(InkMathMode::Confirm);
    let context = ContextToken::capture(&app.session.document);
    let strokes = vec![vec![StrokePoint {
        x: 10.0,
        y: 20.0,
        time: 0.0,
        pressure: 1.0,
    }]];
    let now = Instant::now();
    app.auto_gate = AutoCalculate::new();
    app.auto_gate.set_enabled(true);
    app.auto_gate
        .strokes_finished(now, context.clone(), &strokes)
        .unwrap();
    let request = app
        .auto_gate
        .poll(now + AutoCalculate::IDLE_DELAY, &context)
        .unwrap();
    app.expression = expression.into();
    app.calculation = Some(request.clone());
    request
}

#[test]
fn changed_math_inputs_and_deleted_objects_discard_inflight_results() {
    for change in 0..8 {
        let mut app = app();
        app.add(text_kind("source"));
        let (tx, rx) = mpsc::channel();
        app.start_math(&egui::Context::default(), move || {
            rx.recv().unwrap();
            Ok(("old".into(), Some(text_kind("old"))))
        });
        match change {
            0 => app.expression = "new expression".into(),
            1 => app.math_bounds.x_min += 1.0,
            2 => app.math_bounds.x_max += 1.0,
            3 => app.math_bounds.y_min += 1.0,
            4 => app.math_bounds.y_max += 1.0,
            5 => app.derivative_at += 1.0,
            6 => {
                let params = serde_json::json!({"document_id": app.session.document.id,
                    "page_id": app.session.document.current_page().id,
                    "expected_revision": app.session.document.revision,
                    "operations": [{"op": "delete", "id": app.session.document.current_page().objects[0].id}]});
                app.incoming_message(
                    Request::new("neo:delete", "objects.apply", params)
                        .unwrap()
                        .into(),
                );
            }
            _ => app.incoming_message(
                Request::new(
                    "neo:new",
                    "document.new",
                    serde_json::json!({"discard_unsaved": true}),
                )
                .unwrap()
                .into(),
            ),
        }
        app.math_result = "new status".into();
        tx.send(()).unwrap();
        drain_workers(&mut app);
        assert_eq!(app.math_result, "new status", "change {change}");
        assert!(
            app.session
                .document
                .current_page()
                .objects
                .iter()
                .all(|o| o.kind != text_kind("old"))
        );
        assert!(app.math_ticket.is_none());
    }
}

#[test]
fn reopening_same_identity_invalidates_workers_and_cached_geometry() {
    let mut app = app();
    let path = std::env::temp_dir().join(format!("board-app-race-{}.neoboard", new_id()));
    app.session.save_document(&path).unwrap();
    let before = ContextToken::capture(&app.session.document);
    let ctx = egui::Context::default();
    let render = |app: &mut BoardApp| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
                ..Default::default()
            },
            |ui| app.canvas(ui),
        );
        output.textures_delta.clear();
        format!("{:?}", output.shapes)
    };
    render(&mut app);
    let empty = render(&mut app);
    // 模拟磁盘快照与当前内存具有相同 ID/revision、内容却不同。
    app.session.document.pages[0].objects.push(BoardObject {
        id: new_id(),
        kind: ObjectKind::Stroke {
            points: vec![StrokePoint {
                x: 40.0,
                y: 40.0,
                time: 0.0,
                pressure: 1.0,
            }],
            style: Default::default(),
        },
    });
    app.renderer.clear();
    assert_ne!(render(&mut app), empty);
    app.textures.insert(
        "asset:test".into(),
        ctx.load_texture(
            "test",
            egui::ColorImage::new([1, 1], vec![Color32::RED]),
            Default::default(),
        ),
    );
    let request = arm_confirmation(&mut app, "1+2");
    let (tx, rx) = mpsc::channel();
    app.start_math(&ctx, move || {
        rx.recv().unwrap();
        Ok(("old".into(), Some(text_kind("old"))))
    });
    let (hwr_tx, hwr_rx) = mpsc::channel();
    app.hwr_worker.start(ctx.clone(), move || {
        hwr_rx.recv().unwrap();
        (request, Err("old hwr".into()).into())
    });
    app.incoming_message(
        Request::new(
            "neo:reopen",
            "document.open",
            serde_json::json!({"path": path, "discard_unsaved": true}),
        )
        .unwrap()
        .into(),
    );
    std::fs::remove_file(&path).unwrap();
    assert_eq!(before, ContextToken::capture(&app.session.document));
    assert!(app.textures.is_empty());
    assert_eq!(render(&mut app), empty);
    assert!(app.calculation.is_none());
    app.math_result = "new status".into();
    tx.send(()).unwrap();
    hwr_tx.send(()).unwrap();
    drain_workers(&mut app);
    assert_eq!(app.math_result, "new status");
    assert!(app.session.document.current_page().objects.is_empty());
}

#[test]
fn hwr_confirmation_is_one_shot_and_stale_confirmation_never_inserts() {
    let mut app = app();
    let ctx = egui::Context::default();
    let id = recognize_sample_at(&mut app, &ctx, Vec2::ZERO);
    app.expression = "1+2".into();
    let request = app.calculation.clone().unwrap();
    app.confirm_calculation(&ctx);
    app.confirm_calculation(&ctx);
    drain_workers(&mut app);
    assert_eq!(app.session.document.current_page().objects.len(), 5);
    app.calculation = Some(request);
    app.activate_candidate(&id);
    app.confirm_calculation(&ctx);
    drain_workers(&mut app);
    assert_eq!(app.session.document.current_page().objects.len(), 5);
    recognize_sample_at(&mut app, &ctx, Vec2::new(400.0, 220.0));
    app.session.document.add_page().unwrap();
    app.confirm_calculation(&ctx);
    assert!(!app.math_worker.busy());
    assert!(app.session.document.current_page().objects.is_empty());
}

#[test]
fn math_domain_and_biquadratic_errors_do_not_write_or_reuse_previous_success() {
    let mut app = app();
    let ctx = egui::Context::default();
    let id = recognize_sample_at(&mut app, &ctx, Vec2::ZERO);
    for input in ["1/0", "x^3+y=1;x-y=0", "x^2+y^2=1;sin(x)+y=0"] {
        let error = features::calculation_output(input, app.math_bounds, false)
            .unwrap_err()
            .to_string();
        app.math_result = "previous success".into();
        assert!(app.activate_candidate(&id));
        app.expression = input.into();
        app.confirm_calculation(&ctx);
        drain_workers(&mut app);
        assert_eq!(
            app.session.document.current_page().objects.len(),
            4,
            "{input}"
        );
        assert_eq!(app.math_result, error);
    }
    let integral_error = board_math::integrate("1/x", -1.0, 1.0, Default::default())
        .unwrap_err()
        .to_string();
    let derivative_error = board_math::derivative("sqrt(x)", -1.0)
        .unwrap_err()
        .to_string();
    app.expression = "1/x".into();
    app.start_math(&ctx, || {
        board_math::integrate("1/x", -1.0, 1.0, Default::default())
            .map(|value| (value.to_string(), None))
            .map_err(|e| e.to_string())
    });
    drain_workers(&mut app);
    assert_eq!(app.math_result, integral_error);
    assert_eq!(app.session.document.current_page().objects.len(), 4);
    app.start_math(&ctx, || {
        board_math::derivative("sqrt(x)", -1.0)
            .map(|value| (value.to_string(), None))
            .map_err(|e| e.to_string())
    });
    drain_workers(&mut app);
    assert_eq!(app.math_result, derivative_error);
    assert_eq!(app.session.document.current_page().objects.len(), 4);
}
