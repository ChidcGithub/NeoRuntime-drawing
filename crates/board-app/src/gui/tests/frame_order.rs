fn frame_key(key: Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

// Background sends its result before requesting repaint: this handshake proves
// the result is queued before the input frame, without sleeps or polling it away.
fn worker_ready_context() -> (egui::Context, mpsc::Receiver<()>) {
    let ctx = egui::Context::default();
    for _ in 0..4 {
        let mut output = ctx.run_ui(Default::default(), |_| {});
        output.textures_delta.clear();
    }
    let (tx, rx) = mpsc::channel();
    ctx.set_request_repaint_callback(move |_| {
        let _ = tx.send(());
    });
    (ctx, rx)
}

fn ready_math(app: &mut BoardApp, _ctx: &egui::Context) {
    let (ctx, rx) = worker_ready_context();
    app.start_math(&ctx, || {
        Ok(("ready answer".into(), Some(text_kind("ready answer"))))
    });
    rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(app.math_worker.busy());
}

#[test]
fn logic_before_ui_preserves_manual_math_with_hwr_off_or_confirm_without_new_ink() {
    for mode in [InkMathMode::Off, InkMathMode::Confirm] {
        let mut app = default_app();
        app.set_ink_math_mode(mode);
        let ctx = egui::Context::default();
        board_frame(&mut app, &ctx, vec![]);
        let (tx, rx) = mpsc::channel();
        let (worker_ctx, ready) = worker_ready_context();
        app.start_math(&worker_ctx, move || {
            rx.recv_timeout(Duration::from_secs(3)).unwrap();
            Ok(("5".into(), Some(text_kind("5"))))
        });
        let ticket = app.math_ticket.clone();
        for _ in 0..4 {
            board_frame(&mut app, &ctx, vec![]);
            assert_eq!(app.math_ticket, ticket);
            assert!(app.math_worker.busy());
            assert!(!app.hwr_worker.busy());
        }
        tx.send(()).unwrap();
        ready.recv_timeout(Duration::from_secs(3)).unwrap();
        app.prepare_workers(&ctx, Instant::now());
        assert_eq!(app.math_ticket, ticket, "logic must not cancel manual math");
        assert!(
            app.session.document.current_page().objects.is_empty(),
            "logic must not deliver visible results"
        );
        board_frame(&mut app, &ctx, vec![]);
        assert!(!app.math_worker.busy());
        assert!(matches!(
            app.session.document.current_page().objects[0].kind,
            ObjectKind::Handwritten { .. }
        ));
        for _ in 0..4 {
            board_frame(&mut app, &ctx, vec![]);
        }
        assert_eq!(app.session.document.current_page().objects.len(), 1);
    }
}

#[test]
fn logic_before_ui_does_not_submit_pending_ink_over_manual_math() {
    let mut app = app();
    app.set_ink_math_mode(InkMathMode::Confirm);
    let now = Instant::now();
    write_sample(&mut app, now);
    app.expression = "manual expression".into();
    let ctx = egui::Context::default();
    ready_math(&mut app, &ctx);
    let ticket = app.math_ticket.clone();
    app.prepare_workers(&ctx, now + AutoCalculate::IDLE_DELAY);
    assert_eq!(app.expression, "manual expression");
    assert_eq!(app.math_ticket, ticket);
    assert!(!app.hwr_worker.busy());
    board_frame(&mut app, &ctx, vec![]);
    assert!(!app.math_worker.busy());
    assert_eq!(app.session.document.current_page().objects.len(), 5);
}

#[test]
fn full_board_frame_escape_beats_ready_math_hwr_and_plot() {
    let mut app = app();
    let ctx = egui::Context::default();
    board_frame(&mut app, &ctx, vec![]);
    let request = arm_confirmation(&mut app, "1+1");
    app.ink_ticket = Some(request.id().into());
    let (worker_ctx, rx) = worker_ready_context();
    app.hwr_worker.start(worker_ctx, move || {
        (request, Ok(candidate("stale recognition", 0.9)).into())
    });
    rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let object = drag_plot("plot", 100.0, &["x", "x^2"]);
    app.session.document.pages[0].objects.push(object.clone());
    app.selected = Some(object.id.clone());
    let context = ContextToken::capture(&app.session.document);
    app.plot_points = Some(PlotState {
        context: context.clone(),
        object: object.clone(),
        report: None,
        coordinate: None,
    });
    let (worker_ctx, rx) = worker_ready_context();
    app.plot_worker.start(worker_ctx, move || {
        let report = features::intersections(&object);
        (context, object, report)
    });
    rx.recv_timeout(Duration::from_secs(3)).unwrap();
    ready_math(&mut app, &ctx);
    let document = app.session.document.clone();
    board_frame(
        &mut app,
        &ctx,
        vec![frame_key(Key::Escape, Default::default())],
    );
    assert_eq!(app.session.document, document);
    assert!(app.recognition.is_none());
    assert!(!app.math_worker.busy());
    assert!(!app.hwr_worker.busy());
    // The selected plot may start a fresh search, but cannot accept the old one.
    assert!(
        app.plot_points
            .as_ref()
            .is_none_or(|state| state.report.is_none())
    );
    drain_workers(&mut app);
}

fn full_board_no_animation() -> (BoardApp, egui::Context) {
    let mut app = default_app();
    app.math = true;
    let ctx = egui::Context::default();
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
    }
    (app, ctx)
}

fn full_board_reveal(app: &mut BoardApp, ctx: &egui::Context, text: &str) -> Pos2 {
    fn locate(shape: &egui::Shape, text: &str) -> Option<Rect> {
        match shape {
            egui::Shape::Text(t) if t.galley.text() == text => {
                Some(Rect::from_min_size(t.pos, t.galley.size()))
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| locate(s, text)),
            _ => None,
        }
    }
    for _ in 0..4 {
        board_frame(app, ctx, vec![]);
    }
    for _ in 0..30 {
        let out = board_frame(app, ctx, vec![]);
        if let Some(rect) = out.shapes.iter().find_map(|s| {
            locate(&s.shape, text)
                .filter(|r| s.clip_rect.intersect(ctx.content_rect()).contains_rect(*r))
        }) {
            return rect.center();
        }
        board_frame(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(Pos2::new(250.0, 400.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, -120.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: Default::default(),
                },
            ],
        );
        for _ in 0..30 {
            board_frame(app, ctx, vec![]);
        }
    }
    panic!("full board control not visible: {text}");
}

fn full_board_click(app: &mut BoardApp, ctx: &egui::Context, pos: Pos2) {
    board_frame(
        app,
        ctx,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
    );
    board_frame(app, ctx, vec![pointer(pos, false)]);
}

fn full_board_focus(app: &mut BoardApp, ctx: &egui::Context, pos: Pos2) {
    for _ in 0..100 {
        if ctx
            .memory(|m| m.focused())
            .and_then(|id| ctx.read_response(id))
            .is_some_and(|r| r.rect.contains(pos))
        {
            return;
        }
        board_frame(app, ctx, vec![frame_key(Key::Tab, Default::default())]);
    }
    panic!("cannot focus widget at {pos:?}");
}

#[test]
fn full_board_frame_typing_beats_ready_answer() {
    for event in [
        egui::Event::Text("6".into()),
        egui::Event::Paste("7".into()),
        egui::Event::Cut,
    ] {
        let (mut app, ctx) = full_board_no_animation();
        app.expression = "12345".into();
        let pos = full_board_reveal(&mut app, &ctx, "12345");
        full_board_click(&mut app, &ctx, pos);
        assert!(ctx.egui_wants_keyboard_input());
        board_frame(
            &mut app,
            &ctx,
            vec![frame_key(Key::A, egui::Modifiers::COMMAND)],
        );
        ready_math(&mut app, &ctx);
        board_frame(&mut app, &ctx, vec![event]);
        assert_ne!(app.expression, "12345");
        assert!(app.session.document.current_page().objects.is_empty());
        assert!(!app.math_worker.busy());
    }
}

#[test]
fn full_board_frame_keyboard_and_pointer_settings_beat_ready_answer() {
    for reset in [false, true] {
        for keyboard in [false, true] {
            let (mut app, ctx) = full_board_no_animation();
            app.handwriting.profile = handwriting_answer_profile("5");
            click_text(&mut app, &ctx, "个人笔迹答案（实验性）");
            if reset {
                let pos =
                    full_board_reveal(&mut app, &ctx, "确认清空整个个人笔迹档案（含风格统计）");
                full_board_click(&mut app, &ctx, pos);
            }
            let label = if reset {
                "清空全部样本与风格"
            } else {
                "自动学习并使用个人笔迹（默认开启）"
            };
            let pos = full_board_reveal(&mut app, &ctx, label);
            if keyboard {
                full_board_focus(&mut app, &ctx, pos);
                ready_math(&mut app, &ctx);
                board_frame(
                    &mut app,
                    &ctx,
                    vec![frame_key(Key::Space, Default::default())],
                );
            } else {
                board_frame(
                    &mut app,
                    &ctx,
                    vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
                );
                ready_math(&mut app, &ctx);
                board_frame(&mut app, &ctx, vec![pointer(pos, false)]);
            }
            if reset {
                assert_eq!(app.handwriting.profile.counts(), (0, 0));
            } else {
                assert!(!app.handwriting.enabled);
            }
            assert!(
                app.session.document.current_page().objects.is_empty(),
                "reset={reset}, keyboard={keyboard}"
            );
            assert!(!app.math_worker.busy());
        }
    }
}

#[test]
fn full_board_ready_answer_is_not_starved_by_held_pointer() {
    let (mut app, ctx) = full_board_no_animation();
    let pos = full_board_reveal(&mut app, &ctx, "个人笔迹答案（实验性）");
    board_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
    );
    ready_math(&mut app, &ctx);
    board_frame(&mut app, &ctx, vec![]);
    assert!(ctx.input(|i| i.pointer.any_down()));
    assert!(!app.math_worker.busy());
    assert_eq!(app.session.document.current_page().objects.len(), 1);
}

#[test]
fn full_board_sampling_ctrl_z_precedes_document_undo_without_canvas_learning() {
    let (mut app, ctx) = full_board_no_animation();
    app.add(text_kind("document undo sentinel"));
    let document = app.session.document.clone();
    let profile = app.handwriting.profile.clone();
    click_text(&mut app, &ctx, "个人笔迹答案（实验性）");
    let pos = full_board_reveal(&mut app, &ctx, "可选：精确字形采样");
    full_board_click(&mut app, &ctx, pos);
    let label = full_board_reveal(&mut app, &ctx, "单字符标签（修改标签会清空草稿）");
    full_board_click(&mut app, &ctx, label + Vec2::new(0.0, 24.0));
    assert!(ctx.egui_wants_keyboard_input());
    board_frame(&mut app, &ctx, vec![egui::Event::Text("5".into())]);
    let baseline = full_board_reveal(&mut app, &ctx, "基线 96");
    let start = baseline + Vec2::new(80.0, -35.0);
    full_board_click(&mut app, &ctx, start);
    assert!(app.handwriting.wants_sampling_undo(&ctx));
    assert_eq!(app.session.document, document);
    assert_eq!(app.handwriting.profile, profile);
    assert!(app.gesture.is_none());
    board_frame(
        &mut app,
        &ctx,
        vec![frame_key(Key::Z, egui::Modifiers::CTRL)],
    );
    assert!(!app.handwriting.wants_sampling_undo(&ctx));
    assert_eq!(app.session.document, document);
    assert_eq!(app.handwriting.profile, profile);
    assert!(app.session.history.can_undo());
    // Closing the panel returns Ctrl-Z to document history.
    board_frame(
        &mut app,
        &ctx,
        vec![frame_key(Key::Escape, Default::default())],
    );
    board_frame(
        &mut app,
        &ctx,
        vec![frame_key(Key::Z, egui::Modifiers::CTRL)],
    );
    assert!(app.session.document.current_page().objects.is_empty());
}

#[test]
fn full_board_ready_answer_is_cancelled_by_canvas_press_and_hidden_frames_drain() {
    for hidden in [false, true] {
        let mut app = app();
        let ctx = egui::Context::default();
        board_frame(&mut app, &ctx, vec![]);
        ready_math(&mut app, &ctx);
        if hidden {
            app.set_collapsed(&ctx, true);
        }
        let pos = Pos2::new(700.0, 300.0);
        board_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
        );
        assert!(!app.math_worker.busy());
        assert!(app.session.document.current_page().objects.is_empty());
    }
}

#[test]
fn full_board_handwriting_preview_survives_idle_but_not_context_changes() {
    fn has_preview(output: &egui::FullOutput) -> bool {
        output.shapes.iter().any(|s| {
            matches!(&s.shape, egui::Shape::Rect(r)
            if r.fill == Color32::WHITE && (r.rect.height() - 112.0).abs() < 0.1)
        })
    }
    for change in 0..4 {
        let (mut app, ctx) = full_board_no_animation();
        click_text(&mut app, &ctx, "个人笔迹答案（实验性）");
        let pos = full_board_reveal(&mut app, &ctx, "生成风格预览");
        full_board_click(&mut app, &ctx, pos);
        let deadline = Instant::now() + Duration::from_secs(3);
        while !has_preview(&board_frame(&mut app, &ctx, vec![])) {
            assert!(Instant::now() < deadline, "preview did not become visible");
            std::thread::yield_now();
        }
        for _ in 0..4 {
            assert!(has_preview(&board_frame(&mut app, &ctx, vec![])));
        }
        let document = app.session.document.clone();
        let profile = app.handwriting.profile.clone();
        match change {
            0 => app.expression = "new source".into(),
            1 => {
                app.session.document.add_page().unwrap();
            }
            2 => app.replace_export_resources(Default::default()),
            _ => {
                board_frame(
                    &mut app,
                    &ctx,
                    vec![frame_key(Key::Escape, Default::default())],
                );
                app.math = true;
            }
        };
        assert!(
            !has_preview(&board_frame(&mut app, &ctx, vec![])),
            "change {change}"
        );
        if change != 1 {
            assert_eq!(app.session.document, document);
        }
        assert_eq!(app.handwriting.profile, profile);
    }
}

#[test]
fn font_read_is_bounded_and_invalid_font_never_reaches_egui() {
    let path = std::env::temp_dir().join(format!("board-font-{}", new_id()));
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(board_render::MAX_FONT_BYTES as u64 + 1)
        .unwrap();
    drop(file);
    assert!(read_font(&path).is_err());
    std::fs::write(&path, b"not a font").unwrap();
    let bytes = read_font(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let ctx = egui::Context::default();
    let mut resources = board_render::RenderResources::new();
    assert!(install_font(&ctx, &mut resources, bytes).is_err());
    assert!(resources.handwriting_font().is_none());
    let mut output = ctx.run_ui(Default::default(), |ui| {
        ui.label("default font remains usable");
    });
    output.textures_delta.clear();
}
