#[test]
fn toolbar_three_islands_have_unblocked_gaps_and_block_frame_margins() {
    let mut app = app();
    let ctx = egui::Context::default();
    for size in [Vec2::new(320.0, 480.0), Vec2::new(1280.0, 720.0)] {
        for _ in 0..3 {
            frame(&mut app, &ctx, size, vec![], true);
        }
        let [center, left, right] = app.controls[..] else {
            panic!("three islands")
        };
        assert!((center.center().x - size.x / 2.0).abs() < 1.0);
        let edge = 8.0 * toolbar_scale(size, false);
        assert!((left.left() - edge).abs() < 1.0);
        assert!((right.right() - (size.x - edge)).abs() < 1.0);
        assert!(left.right() + 4.0 < center.left());
        assert!(center.right() + 4.0 < right.left());
        for island in [left, center, right] {
            let pos = island.left_top() + Vec2::splat(3.0);
            let before = app.session.document.clone();
            frame(
                &mut app,
                &ctx,
                size,
                vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
                true,
            );
            frame(&mut app, &ctx, size, vec![pointer(pos, false)], true);
            assert!(app.gesture.is_none());
            assert_eq!(app.session.document, before);
        }
        for (a, b) in [(left, center), (center, right)] {
            let pos = Pos2::new((a.right() + b.left()) / 2.0, center.center().y);
            assert!(!app.controls.iter().any(|r| r.contains(pos)));
            let before = app.session.document.current_page().objects.len();
            frame(
                &mut app,
                &ctx,
                size,
                vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
                true,
            );
            frame(
                &mut app,
                &ctx,
                size,
                vec![egui::Event::PointerMoved(pos + Vec2::new(0.0, -12.0))],
                true,
            );
            frame(
                &mut app,
                &ctx,
                size,
                vec![pointer(pos + Vec2::new(0.0, -12.0), false)],
                true,
            );
            assert_eq!(
                app.session.document.current_page().objects.len(),
                before + 1
            );
        }
    }
}

#[test]
fn toolbar_relative_scale_edge_anchors_and_resize_keep_real_targets_usable() {
    for mode in [AppMode::Blackboard, AppMode::Drawing] {
        let mut app = app();
        app.mode = mode;
        let ctx = egui::Context::default();
        let original_style = ctx.style_of(egui::Theme::Dark);
        for (size, expected_scale, dpi) in [
            (Vec2::new(480.0, 480.0), 0.8, 1.0),
            (Vec2::new(640.0, 480.0), 0.8, 1.25),
            (Vec2::new(1280.0, 720.0), 0.8, 1.5),
            (Vec2::new(1920.0, 1080.0), 1.0, 1.0),
            (Vec2::new(2560.0, 1440.0), 4.0 / 3.0, 1.5),
            (Vec2::new(3840.0, 2160.0), 1.5, 2.0),
            (Vec2::new(1920.0, 1080.0), 1.0, 2.0),
            (Vec2::new(1920.0, 720.0), 0.8, 1.0),
            (Vec2::new(2560.0, 1080.0), 1.0, 2.0),
            (Vec2::new(480.0, 480.0), 0.8, 2.0),
        ] {
            let viewport = Rect::from_min_size(Pos2::new(91.0, 37.0), size);
            let render = |app: &mut BoardApp, events| {
                let mut input = egui::RawInput {
                    screen_rect: Some(viewport),
                    events,
                    ..Default::default()
                };
                input
                    .viewports
                    .get_mut(&egui::ViewportId::ROOT)
                    .unwrap()
                    .native_pixels_per_point = Some(dpi);
                let mut out = ctx.run_ui(input, |ui| app.board_ui(ui, &ctx));
                out.textures_delta.clear();
                out
            };
            for _ in 0..5 {
                render(&mut app, vec![]);
            }
            let output = render(&mut app, vec![]);
            let center = app.controls[0];
            let left = app.controls[1];
            let right = app.controls[2];
            assert!((ctx.pixels_per_point() - dpi).abs() < 0.01);
            let scale = expected_scale * 1.1;
            let expected = 42.0 * scale;
            assert!(
                (center.center().x - viewport.center().x).abs() < 1.0,
                "{size:?}: {center:?}"
            );
            assert!((left.left() - viewport.left() - 8.0 * scale).abs() < 1.0);
            assert!((viewport.right() - right.right() - 8.0 * scale).abs() < 1.0);
            assert!(left.right() + 4.0 < center.left() && center.right() + 4.0 < right.left());
            for island in [center, left, right] {
                assert!(viewport.contains_rect(island));
                assert!((island.height() - center.height()).abs() < 1.0);
                assert!((island.bottom() - center.bottom()).abs() < 1.0);
            }
            if mode == AppMode::Drawing {
                assert!(center.bottom() < app.controls[3].top());
            }
            let icon_rect = |label| {
                ctx.data(|d| d.get_temp::<Rect>(egui::Id::new(("icon_rect", label))))
                    .unwrap()
            };
            let more = icon_rect("更多 / 状态");
            let collapse = icon_rect("隐藏工具栏（保留板书）");
            for label in [
                "画笔",
                "橡皮",
                "选择 / 顶点编辑",
                "线段",
                "更多 / 状态",
                "隐藏工具栏（保留板书）",
                "上一页",
                "添加页面",
            ] {
                let rect = icon_rect(label);
                assert!(
                    (rect.width() - expected).abs() < 1.0 && (rect.height() - expected).abs() < 1.0,
                    "{label}: {rect:?}"
                );
            }
            assert!(center.contains_rect(more) && center.contains_rect(collapse));
            assert!((collapse.left() - more.right() - 4.0 * scale).abs() < 1.0);
            assert!(
                (center.right() - collapse.right() - ((6.0 * scale).round() + scale)).abs() < 1.0
            );
            let page = ctx
                .data(|d| d.get_temp::<Rect>(egui::Id::new("page_count_rect")))
                .unwrap();
            assert!((page.width() - 56.0 * scale).abs() < 1.0);
            assert!((page.height() - expected).abs() < 1.0);
            assert!(right.contains_rect(page));
            let text = output
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.text() == "1/1" => Some(t),
                    _ => None,
                })
                .unwrap();
            assert!((text.galley.job.sections[0].format.font_id.size - 12.0 * scale).abs() < 0.01);
            assert_eq!(
                ctx.style_of(egui::Theme::Dark).text_styles,
                original_style.text_styles
            );
            assert_eq!(
                ctx.style_of(egui::Theme::Dark).spacing.interact_size,
                original_style.spacing.interact_size
            );
            // Fixed end buttons remain clickable after resizing; neither click draws ink.
            let pos = more.center();
            render(
                &mut app,
                vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
            );
            render(&mut app, vec![pointer(pos, false)]);
            assert!(egui::Popup::is_any_open(&ctx));
            assert!(app.gesture.is_none());
            egui::Popup::close_all(&ctx);
            for _ in 0..3 {
                render(&mut app, vec![]);
            }
            let pos = collapse.center();
            render(
                &mut app,
                vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
            );
            render(&mut app, vec![pointer(pos, false)]);
            assert!(app.toolbar_hidden && !app.collapsed);
            for _ in 0..4 {
                render(&mut app, vec![]);
            }
            let expand = icon_rect("显示工具栏");
            assert!((expand.width() - expected).abs() < 1.0);
            let pos = expand.center();
            render(
                &mut app,
                vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
            );
            render(&mut app, vec![pointer(pos, false)]);
            assert!(!app.toolbar_hidden);
            assert!(app.session.document.current_page().objects.is_empty());
        }
    }
}

#[test]
fn toolbar_scrolls_only_center_and_keeps_end_tools_reachable() {
    let mut app = app();
    let ctx = egui::Context::default();
    let size = Vec2::new(480.0, 480.0);
    for _ in 0..3 {
        frame(&mut app, &ctx, size, vec![], true);
    }
    let [center, left, right] = app.controls[..] else {
        panic!("three islands")
    };
    let more = ctx
        .data(|d| d.get_temp::<Rect>(egui::Id::new(("icon_rect", "更多 / 状态"))))
        .unwrap();
    let collapse = ctx
        .data(|d| d.get_temp::<Rect>(egui::Id::new(("icon_rect", "隐藏工具栏（保留板书）"))))
        .unwrap();
    assert!(center.contains_rect(more) && center.contains_rect(collapse));
    assert!(more.right() < collapse.left());
    let pos = Pos2::new(center.left() + 15.0, center.center().y);
    frame(
        &mut app,
        &ctx,
        size,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: Vec2::new(-1200.0, 0.0),
                phase: egui::TouchPhase::Move,
                modifiers: Default::default(),
            },
        ],
        true,
    );
    for _ in 0..30 {
        frame(&mut app, &ctx, size, vec![], true);
    }
    assert_eq!(app.controls, vec![center, left, right]);
    let rect = ctx
        .data(|d| d.get_temp::<Rect>(egui::Id::new(("icon_rect", "数学 / 模型 / 截图 / AI"))))
        .unwrap();
    assert!(center.contains_rect(rect), "{center:?}: {rect:?}");
    assert!(rect.right() < more.left());
    let pos = rect.center();
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
        true,
    );
    frame(&mut app, &ctx, size, vec![pointer(pos, false)], true);
    assert!(egui::Popup::is_any_open(&ctx));
    for _ in 0..3 {
        frame(&mut app, &ctx, size, vec![], true);
    }
    let out = frame(&mut app, &ctx, size, vec![], true);
    text_position(&out, "数学 / 手写识别 / 模型设置");
    assert!(egui::Popup::is_any_open(&ctx));
    assert!(app.gesture.is_none());
    assert!(app.session.document.current_page().objects.is_empty());
}

#[test]
fn toolbar_last_page_plus_adds_selects_undoes_and_respects_bounds() {
    let mut app = app();
    let ctx = egui::Context::default();
    let first = app.session.document.current_page().id.clone();
    let before = app.session.document.clone();
    click_toolbar_icon(&mut app, &ctx, "上一页");
    assert_eq!(app.session.document, before);
    assert!(!app.session.history.can_undo());
    click_toolbar_icon(&mut app, &ctx, "添加页面");
    assert_eq!(app.session.document.pages.len(), 2);
    assert_eq!(app.session.document.current_page, 1);
    let second = app.session.document.current_page().id.clone();
    assert_ne!(first, second);
    assert!(app.session.document.current_page().objects.is_empty());
    click_toolbar_icon(&mut app, &ctx, "撤销");
    assert_eq!(app.session.document.pages.len(), 1);
    assert_eq!(app.session.document.current_page().id, first);
    assert!(!app.session.history.can_undo());
    click_toolbar_icon(&mut app, &ctx, "重做");
    assert_eq!(app.session.document.current_page().id, second);
    click_toolbar_icon(&mut app, &ctx, "上一页");
    let revision = app.session.document.revision;
    click_toolbar_icon(&mut app, &ctx, "下一页");
    assert_eq!(app.session.document.current_page().id, second);
    assert_eq!(app.session.document.revision, revision);
    for _ in 2..board_core::MAX_PAGES {
        app.session.document.add_page().unwrap();
    }
    let before = app.session.document.clone();
    click_toolbar_icon(&mut app, &ctx, "添加页面");
    assert_eq!(app.session.document, before);
    click_toolbar_icon(&mut app, &ctx, "上一页");
    click_toolbar_icon(&mut app, &ctx, "下一页");
    assert_eq!(app.session.document, before);
}

#[test]
fn toolbar_drawing_management_keeps_collapse_and_dirty_exit_semantics() {
    let mut app = app();
    app.mode = AppMode::Drawing;
    let ctx = egui::Context::default();
    write_sample_at(&mut app, Instant::now(), Vec2::ZERO);
    let before = app.session.document.clone();
    open_toolbar_menu(&mut app, &ctx, "画板菜单");
    click_text(&mut app, &ctx, "收起画板工具");
    assert!(app.collapsed && !app.compact_entry());
    assert_eq!(app.session.document, before);
    click_toolbar_icon(&mut app, &ctx, "显示工具栏");
    assert!(!app.collapsed);
    open_toolbar_menu(&mut app, &ctx, "画板菜单");
    click_text(&mut app, &ctx, "退出画板");
    assert!(app.confirm_close && !app.allow_close);
    assert_eq!(app.session.document, before);
    assert!(app.gesture.is_none());
}

#[test]
fn toolbar_continuous_shrink_keeps_center_and_edges_anchored() {
    for mode in [AppMode::Blackboard, AppMode::Drawing] {
        let mut app = app();
        app.mode = mode;
        let ctx = egui::Context::default();
        let mut previous_scale = 1.5 * 1.1;
        for width in (320..=3840).rev().step_by(16) {
            let size = Vec2::new(width as f32, (width as f32 * 9.0 / 16.0).max(240.0));
            for _ in 0..3 {
                frame(&mut app, &ctx, size, vec![], true);
            }
            let scale = toolbar_scale(size, false);
            assert!(scale <= previous_scale && previous_scale - scale < 0.05);
            previous_scale = scale;
            let center = app.controls[0];
            let left = app.controls[1];
            let right = app.controls[2];
            assert!((center.center().x - size.x / 2.0).abs() < 1.0);
            assert!((left.left() - 8.0 * scale).abs() < 1.0);
            assert!((right.right() - size.x + 8.0 * scale).abs() < 1.0);
            assert!(left.right() < center.left() && center.right() < right.left());
            if mode == AppMode::Drawing {
                assert!(center.bottom() < app.controls[3].top());
            }
            for label in ["更多 / 状态", "隐藏工具栏（保留板书）"] {
                let rect = ctx
                    .data(|d| d.get_temp::<Rect>(egui::Id::new(("icon_rect", label))))
                    .unwrap();
                assert!(center.expand(0.5).contains_rect(rect));
            }
        }
    }
}

#[test]
fn toolbar_survives_virtual_viewport_shrinking_without_negative_dimensions() {
    let mut app = app();
    let ctx = egui::Context::default();
    for size in [
        Vec2::new(1920.0, 1080.0),
        Vec2::new(480.0, 320.0),
        Vec2::new(321.0, 240.0),
        Vec2::new(320.0, 240.0),
        Vec2::new(319.0, 240.0),
        Vec2::new(80.0, 80.0),
        Vec2::new(800.0, 600.0),
    ] {
        for _ in 0..3 {
            frame(&mut app, &ctx, size, vec![], true);
        }
        assert_eq!(app.controls.len(), if size.x < 320.0 { 1 } else { 3 });
        for rect in &app.controls {
            assert!(rect.is_finite() && rect.is_positive());
            assert!(
                Rect::from_min_size(Pos2::ZERO, size)
                    .expand(1.0)
                    .contains_rect(*rect),
                "{size:?}: {rect:?}"
            );
        }
        let rect = app.controls[0];
        assert!(rect.is_finite() && rect.is_positive());
        let scale = toolbar_scale(size, size.x < 320.0);
        let height = 44.0 * scale + 2.0 * (6.0 * scale).round();
        assert!((rect.height() - height).abs() < 1.0, "{rect:?}");
        assert!(
            Rect::from_min_size(Pos2::ZERO, size)
                .expand(1.0)
                .contains_rect(rect),
            "{size:?}: {rect:?}"
        );
    }
}

#[test]
fn collapsed_drawing_keeps_ink_blackboard_only_entry_preserves_history() {
    let ink_color = Color32::from_rgb(123, 45, 67);
    let ink_shapes = |output: &egui::FullOutput| {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) if mesh.vertices.iter().any(|v| v.color == ink_color) => {
                    assert!(mesh.vertices.iter().all(|v| {
                        (v.color == ink_color || v.color == Color32::TRANSPARENT)
                            && (45.0..=155.0).contains(&v.pos.x)
                            && (45.0..=85.0).contains(&v.pos.y)
                    }));
                    Some(mesh.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    for mode in [AppMode::Drawing, AppMode::Blackboard] {
        let mut app = app();
        app.mode = mode;
        app.add(ObjectKind::Stroke {
            points: vec![
                StrokePoint {
                    x: 50.0,
                    y: 50.0,
                    time: 0.0,
                    pressure: 1.0,
                },
                StrokePoint {
                    x: 150.0,
                    y: 80.0,
                    time: 0.2,
                    pressure: 1.0,
                },
            ],
            style: Style {
                color: core_color(ink_color),
                width: 4.0,
                dashed: false,
            },
        });
        let before = app.session.document.clone();
        let ctx = egui::Context::default();
        board_frame(&mut app, &ctx, vec![]);
        let expanded_ink = ink_shapes(&board_frame(&mut app, &ctx, vec![]));
        assert!(
            !expanded_ink.is_empty(),
            "expanded canvas must actually draw the stroke"
        );
        app.files = true;
        app.math = true;
        app.authorization = Some(true);
        let mut out = ctx.run_ui(egui::RawInput::default(), |_| app.set_collapsed(&ctx, true));
        out.textures_delta.clear();
        assert!(!app.files && !app.math && app.authorization.is_none());
        assert!(app.gesture.is_none() && app.ink_context.is_none());
        assert_eq!(app.session.document, before);
        let commands = &out.viewport_output[&egui::ViewportId::ROOT].commands;
        assert_eq!(
            commands.iter().any(
                |c| matches!(c, ViewportCommand::InnerSize(s) if *s == Vec2::new(220.0, 64.0))
            ),
            mode == AppMode::Blackboard
        );
        board_frame(&mut app, &ctx, vec![]);
        let out = board_frame(&mut app, &ctx, vec![]);
        let shapes = format!("{:?}", out.shapes);
        assert!(!shapes.contains("本次截图授权"));
        assert!(!shapes.contains("上一页") && !shapes.contains("线段"));
        assert_eq!(app.controls.len(), 1);
        if mode == AppMode::Blackboard {
            text_position(&out, "退出");
            assert!(ink_shapes(&out).is_empty());
            text_position(&out, "展开黑板");
            // Entry exit restores the normal window for the dirty confirmation.
            click_text(&mut app, &ctx, "退出");
            assert!(app.confirm_close && !app.allow_close && !app.collapsed);
        } else {
            assert_eq!(ink_shapes(&out), expanded_ink);
            assert!(app.tool == Tool::Pen && !app.passthrough);
            assert!(
                commands.is_empty(),
                "Drawing collapse must not resize the viewport"
            );
            let scale = toolbar_scale(Vec2::new(1280.0, 720.0), true);
            let height = 44.0 * scale + 2.0 * (6.0 * scale).round();
            assert!((app.controls[0].height() - height).abs() < 1.0);
            click_toolbar_icon(&mut app, &ctx, "显示工具栏");
            assert!(!app.collapsed);
            assert_eq!(
                ink_shapes(&board_frame(&mut app, &ctx, vec![])),
                expanded_ink
            );
            open_toolbar_menu(&mut app, &ctx, "画板菜单");
            click_text(&mut app, &ctx, "收起画板工具");
            click_toolbar_icon(&mut app, &ctx, "退出（未保存时确认）");
            assert!(app.confirm_close && !app.allow_close && !app.collapsed);
        }
        assert_eq!(app.session.document, before);
        app.undo(false);
        assert!(app.session.document.current_page().objects.is_empty());
    }
}
