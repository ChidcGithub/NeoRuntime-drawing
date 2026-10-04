#[test]
fn toolbar_only_pen_and_eraser_require_two_clicks_each_time_in_both_modes() {
    for mode in [AppMode::Drawing, AppMode::Blackboard] {
        let mut app = app();
        app.mode = mode;
        let ctx = egui::Context::default();
        let management = if mode == AppMode::Drawing {
            "画板菜单"
        } else {
            "黑板菜单"
        };
        for label in [
            "画笔",
            "橡皮",
            "其他形状",
            "文件 / 保存 / 载入 / 导出",
            "数学 / 模型 / 截图 / AI",
            "更多 / 状态",
            management,
        ] {
            open_toolbar_menu(&mut app, &ctx, label);
            click_toolbar_icon(&mut app, &ctx, label);
            assert!(!egui::Popup::is_any_open(&ctx), "click closes: {label}");
            open_toolbar_menu(&mut app, &ctx, label);
            egui::Popup::close_all(&ctx);
        }
        for expected_open in [true, false, true, false] {
            click_text(&mut app, &ctx, "1/1");
            assert_eq!(egui::Popup::is_any_open(&ctx), expected_open);
            assert!(app.armed_toolbar_menu.is_none());
        }
        assert!(app.session.document.current_page().objects.is_empty());
        assert!(!app.session.history.can_undo());
    }
}

#[test]
fn toolbar_menu_first_click_is_reset_by_other_controls_canvas_and_escape() {
    let mut app = app();
    let ctx = egui::Context::default();
    click_toolbar_icon(&mut app, &ctx, "画笔");
    assert!(!egui::Popup::is_any_open(&ctx));
    click_toolbar_icon(&mut app, &ctx, "橡皮");
    assert!(!egui::Popup::is_any_open(&ctx));
    open_toolbar_menu(&mut app, &ctx, "画笔");
    egui::Popup::close_all(&ctx);
    click_toolbar_icon(&mut app, &ctx, "画笔");
    click_toolbar_icon(&mut app, &ctx, "选择 / 顶点编辑");
    assert!(app.armed_toolbar_menu.is_none());
    open_toolbar_menu(&mut app, &ctx, "画笔");
    egui::Popup::close_all(&ctx);
    click_toolbar_icon(&mut app, &ctx, "橡皮");
    assert!(app.armed_toolbar_menu.is_some());
    let pos = Pos2::new(100.0, 100.0);
    board_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
    );
    board_frame(&mut app, &ctx, vec![pointer(pos, false)]);
    assert!(app.armed_toolbar_menu.is_none());
    open_toolbar_menu(&mut app, &ctx, "橡皮");
    egui::Popup::close_all(&ctx);
    click_toolbar_icon(&mut app, &ctx, "画笔");
    board_frame(
        &mut app,
        &ctx,
        vec![egui::Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    assert!(app.armed_toolbar_menu.is_none());
    open_toolbar_menu(&mut app, &ctx, "画笔");
    egui::Popup::close_all(&ctx);
    for label in [
        "更多 / 状态",
        "黑板菜单",
        "其他形状",
        "文件 / 保存 / 载入 / 导出",
        "数学 / 模型 / 截图 / AI",
    ] {
        click_toolbar_icon(&mut app, &ctx, "橡皮");
        assert!(app.armed_toolbar_menu.is_some());
        open_toolbar_menu(&mut app, &ctx, label);
        assert!(app.armed_toolbar_menu.is_none());
        egui::Popup::close_all(&ctx);
        open_toolbar_menu(&mut app, &ctx, "橡皮");
        egui::Popup::close_all(&ctx);
    }
    click_toolbar_icon(&mut app, &ctx, "画笔");
    click_text(&mut app, &ctx, "1/1");
    assert!(egui::Popup::is_any_open(&ctx) && app.armed_toolbar_menu.is_none());
    egui::Popup::close_all(&ctx);
    open_toolbar_menu(&mut app, &ctx, "画笔");
    assert!(app.session.document.current_page().objects.is_empty());
}

#[test]
fn actual_line_button_drag_and_geometry_dropdown() {
    let mut app = app();
    let ctx = egui::Context::default();
    click_toolbar_icon(&mut app, &ctx, "线段");
    assert!(app.tool == Tool::Shape && app.shape == ShapeKind::Line);
    let a = Pos2::new(100.0, 100.0);
    let b = Pos2::new(240.0, 100.0);
    board_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(a), pointer(a, true)],
    );
    board_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(b)]);
    board_frame(&mut app, &ctx, vec![pointer(b, false)]);
    assert!(
        matches!(&app.session.document.current_page().objects[0].kind,
        ObjectKind::Shape { shape: ShapeKind::Line, points, .. } if points.len() == 2 && points[0].x == 100.0 && points[1].x == 240.0),
        "{:?}",
        app.session.document.current_page().objects
    );
    open_toolbar_menu(&mut app, &ctx, "其他形状");
    board_frame(&mut app, &ctx, vec![]);
    assert!(app.controls[1].bottom() <= app.controls[0].top() + 8.0);
    click_text(&mut app, &ctx, "长方形");
    assert!(app.tool == Tool::Shape && app.shape == ShapeKind::Rectangle);
}

#[test]
fn vector_icon_hover_selected_disabled_and_accessibility() {
    let ctx = egui::Context::default();
    let render = |selected, enabled, events| {
        let mut response = None;
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 240.0))),
                events,
                ..Default::default()
            },
            |ui| {
                response = Some(icons::button(
                    ui,
                    Icon::Calculator,
                    "计算",
                    28.0,
                    selected,
                    enabled,
                ));
            },
        );
        out.textures_delta.clear();
        (out, response.unwrap())
    };
    render(false, true, vec![]);
    let (normal, response) = render(false, true, vec![]);
    assert_eq!(response.rect.size(), Vec2::splat(28.0));
    let pos = response.rect.center();
    let (hover, response) = render(false, true, vec![egui::Event::PointerMoved(pos)]);
    assert!(response.hovered());
    let (selected, _) = render(true, true, vec![]);
    let (disabled, response) = render(false, false, vec![pointer(pos, true), pointer(pos, false)]);
    assert!(!response.enabled() && !response.clicked());
    assert_ne!(
        format!("{:?}", normal.shapes),
        format!("{:?}", hover.shapes)
    );
    assert_ne!(
        format!("{:?}", selected.shapes),
        format!("{:?}", hover.shapes)
    );
    assert_ne!(
        format!("{:?}", disabled.shapes),
        format!("{:?}", normal.shapes)
    );
    render(false, true, vec![]);
    render(false, true, vec![pointer(pos, true)]);
    let (clicked, response) = render(false, true, vec![pointer(pos, false)]);
    assert!(response.clicked());
    assert!(format!("{:?}", clicked.platform_output.events).contains("计算"));
    assert!(!format!("{:?}", normal.shapes).contains("Text("));
}

#[test]
fn toolbar_menu_properties_history_and_exit_use_real_pointer_clicks() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.tool = Tool::Select;
    click_toolbar_icon(&mut app, &ctx, "画笔");
    assert!(app.tool == Tool::Pen && !egui::Popup::is_any_open(&ctx));
    click_toolbar_icon(&mut app, &ctx, "画笔");
    assert!(egui::Popup::is_any_open(&ctx));
    click_text(&mut app, &ctx, "虚线");
    assert!(app.style.dashed);
    let popup = app.controls[1];
    let red = Pos2::new(popup.left() + 20.0, popup.top() + 40.0);
    board_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(red), pointer(red, true)],
    );
    board_frame(&mut app, &ctx, vec![pointer(red, false)]);
    assert_eq!(app.style.color, core_color(Color32::RED));
    assert!(app.gesture.is_none());
    assert!(app.session.document.current_page().objects.is_empty());
    egui::Popup::close_all(&ctx);
    click_toolbar_icon(&mut app, &ctx, "橡皮");
    assert!(app.tool == Tool::Eraser && !egui::Popup::is_any_open(&ctx));
    click_toolbar_icon(&mut app, &ctx, "橡皮");
    let out = board_frame(&mut app, &ctx, vec![]);
    text_position(&out, "橡皮半径");
    egui::Popup::close_all(&ctx);
    click_toolbar_icon(&mut app, &ctx, "选择 / 顶点编辑");
    assert!(app.tool == Tool::Select);
    click_toolbar_icon(&mut app, &ctx, "画笔");
    assert!(app.tool == Tool::Pen);
    egui::Popup::close_all(&ctx);
    app.add(ObjectKind::Text {
        position: Point { x: 60.0, y: 60.0 },
        text: "保留".into(),
        size: 24.0,
        color: core_color(Color32::WHITE),
    });
    click_toolbar_icon(&mut app, &ctx, "撤销");
    assert!(app.session.document.current_page().objects.is_empty());
    click_toolbar_icon(&mut app, &ctx, "重做");
    assert_eq!(app.session.document.current_page().objects.len(), 1);
    open_toolbar_menu(&mut app, &ctx, "黑板菜单");
    click_text(&mut app, &ctx, "退出黑板");
    assert!(app.confirm_close && !app.allow_close);
    assert_eq!(app.session.document.current_page().objects.len(), 1);
}

#[test]
fn narrow_more_menu_blocks_canvas_and_preserves_single_row() {
    let mut app = app();
    let ctx = egui::Context::default();
    let size = Vec2::new(320.0, 480.0);
    for _ in 0..3 {
        frame(&mut app, &ctx, size, vec![], true);
    }
    let bar = app.controls[0];
    let pos = ctx
        .data(|data| data.get_temp::<Rect>(egui::Id::new(("icon_rect", "更多 / 状态"))))
        .unwrap()
        .center();
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
        true,
    );
    frame(&mut app, &ctx, size, vec![pointer(pos, false)], true);
    assert!(egui::Popup::is_any_open(&ctx));
    frame(&mut app, &ctx, size, vec![], true);
    let out = frame(&mut app, &ctx, size, vec![], true);
    assert!(egui::Popup::is_any_open(&ctx));
    assert_eq!(app.controls[0], bar);
    let painted = format!("{:?}", out.shapes);
    assert!(!painted.contains("收起黑板") && !painted.contains("退出黑板"));
    for label in [
        "其他形状",
        "画笔",
        "选择 / 顶点编辑",
        "撤销",
        "重做",
        "状态与任务",
    ] {
        let p = text_position(&out, label);
        assert!(ctx.content_rect().contains(p), "{label}: {p:?}");
    }
    let pos = text_position(&out, "其他形状");
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
        true,
    );
    frame(&mut app, &ctx, size, vec![pointer(pos, false)], true);
    frame(&mut app, &ctx, size, vec![], true);
    let out = frame(&mut app, &ctx, size, vec![], true);
    let pos = text_position(&out, "长方形");
    assert!(ctx.content_rect().contains(pos));
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
        true,
    );
    frame(&mut app, &ctx, size, vec![pointer(pos, false)], true);
    assert!(app.tool == Tool::Shape && app.shape == ShapeKind::Rectangle);
    assert!(app.gesture.is_none());
    assert!(app.session.document.current_page().objects.is_empty());
}

#[test]
fn toolbar_hide_restore_and_more_collapse_are_independent_pointer_actions() {
    let mut app = app();
    let ctx = egui::Context::default();
    write_sample_at(&mut app, Instant::now(), Vec2::ZERO);
    let before = app.session.document.clone();
    click_toolbar_icon(&mut app, &ctx, "隐藏工具栏（保留板书）");
    assert!(app.toolbar_hidden && !app.collapsed && !app.compact_entry());
    let out = board_frame(&mut app, &ctx, vec![]);
    assert!(
        out.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .is_empty()
    );
    assert!(
        out.shapes
            .iter()
            .any(|s| matches!(s.shape, egui::Shape::Mesh(_)))
    );
    assert_eq!(app.session.document, before);
    click_toolbar_icon(&mut app, &ctx, "显示工具栏");
    assert!(!app.toolbar_hidden && !app.collapsed);
    open_toolbar_menu(&mut app, &ctx, "黑板菜单");
    click_text(&mut app, &ctx, "收起黑板");
    assert!(app.compact_entry() && !app.toolbar_hidden);
    assert_eq!(app.session.document, before);
}

#[test]
fn toolbar_fixed_pages_fit_and_previews_jump_without_drawing_or_losing_cache() {
    let mut app = app();
    let ctx = egui::Context::default();
    let candidate = recognize_sample_at(&mut app, &ctx, Vec2::ZERO);
    app.session
        .history
        .edit(&mut app.session.document, |d| d.add_page())
        .unwrap();
    app.session
        .history
        .edit(&mut app.session.document, |d| d.add_page())
        .unwrap();
    app.add(ObjectKind::Math {
        position: Point { x: 80.0, y: 80.0 },
        layout: board_core::MathLayout::Fraction(
            Box::new(board_core::MathLayout::Text("5".into())),
            Box::new(board_core::MathLayout::Text("6".into())),
        ),
        size: 36.0,
        color: core_color(Color32::YELLOW),
    });
    app.select_page(0);
    let revision = app.session.document.revision;
    for size in [
        Vec2::new(320.0, 480.0),
        Vec2::new(800.0, 600.0),
        Vec2::new(1920.0, 1080.0),
    ] {
        for _ in 0..3 {
            frame(&mut app, &ctx, size, vec![], true);
        }
        let bar = ctx
            .data(|d| d.get_temp::<Rect>(egui::Id::new("board_pages")))
            .unwrap();
        let rect = |label| {
            ctx.data(|d| d.get_temp::<Rect>(egui::Id::new(("icon_rect", label))))
                .unwrap()
        };
        let previous = rect("上一页");
        let next = rect("下一页");
        let scale = toolbar_scale(size, false);
        let button_size = 42.0 * scale;
        assert!((previous.size() - Vec2::splat(button_size)).length() < 1.0);
        assert!((next.size() - Vec2::splat(button_size)).length() < 1.0);
        let count_rect = ctx
            .data(|d| d.get_temp::<Rect>(egui::Id::new("page_count_rect")))
            .unwrap();
        assert!((count_rect.height() - button_size).abs() < 1.0);
        assert!(previous.right() < count_rect.left() && count_rect.right() < next.left());
        assert!(bar.contains_rect(previous) && bar.contains_rect(next));
        assert!((bar.right() - next.right() - ((6.0 * scale).round() + scale)).abs() < 2.0);
        assert!(rect("隐藏工具栏（保留板书）").right() < previous.left());
        assert!(Rect::from_min_size(Pos2::ZERO, size).contains_rect(bar));
    }
    click_text(&mut app, &ctx, "1/3");
    assert!(egui::Popup::is_any_open(&ctx));
    board_frame(&mut app, &ctx, vec![]);
    board_frame(&mut app, &ctx, vec![]);
    let out = board_frame(&mut app, &ctx, vec![]);
    assert!(
        app.thumbnails.len() == 3,
        "cached {} controls {:?}",
        app.thumbnails.len(),
        app.controls
    );
    text_position(&out, "5");
    text_position(&out, "6");
    assert!(app.controls[2].bottom() <= app.controls[0].top() + 8.0);
    assert!(!format!("{:?}", out.shapes).contains("新页"));
    assert!(!app.thumbnails.is_empty());
    assert_eq!(app.session.document.current_page, 0);
    assert_eq!(app.session.document.revision, revision);
    click_text(&mut app, &ctx, "第 3 页");
    assert_eq!(app.session.document.current_page, 2);
    assert!(app.gesture.is_none());
    assert_eq!(app.session.document.revision, revision);
    click_text(&mut app, &ctx, "3/3");
    assert!(egui::Popup::is_any_open(&ctx));
    click_text(&mut app, &ctx, "第 1 页");
    assert!(app.ink_cache.get(&candidate).is_some());
    assert_eq!(app.session.document.current_page().objects.len(), 4);
}

#[test]
fn toolbar_hundreds_of_pages_only_cache_visible_previews_and_report_failure() {
    let mut app = app();
    let ctx = egui::Context::default();
    for _ in 1..500 {
        app.session.document.add_page().unwrap();
    }
    app.select_page(0);
    app.session.document.pages[0].objects.push(BoardObject {
        id: "missing-preview".into(),
        kind: ObjectKind::Image {
            position: Point { x: 10.0, y: 10.0 },
            width: 100.0,
            height: 100.0,
            asset_ref: "asset:missing".into(),
        },
    });
    let size = Vec2::new(320.0, 480.0);
    for _ in 0..3 {
        frame(&mut app, &ctx, size, vec![], true);
    }
    let count_rect = ctx
        .data(|d| d.get_temp::<Rect>(egui::Id::new("page_count_rect")))
        .unwrap();
    let pos = count_rect.center();
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
    text_position(&out, "预览失败");
    assert!(!app.thumbnails.is_empty() && app.thumbnails.len() < 6);
    assert!(Rect::from_min_size(Pos2::ZERO, size).contains_rect(app.controls[2]));
    egui::Popup::close_all(&ctx);
    app.select_page(499);
    for _ in 0..3 {
        frame(&mut app, &ctx, size, vec![], true);
    }
    let count_rect = ctx
        .data(|d| d.get_temp::<Rect>(egui::Id::new("page_count_rect")))
        .unwrap();
    assert!(
        (count_rect.size() - Vec2::new(56.0, 42.0) * toolbar_scale(size, false)).length() < 1.0,
        "{count_rect:?}"
    );
    assert!(Rect::from_min_size(Pos2::ZERO, size).contains_rect(app.controls[0]));
}

#[test]
fn toolbar_blackboard_rejects_passthrough_and_drawing_switch_exits() {
    let mut app = app();
    let ctx = egui::Context::default();
    for (hidden, collapsed) in [(false, false), (true, false), (false, true)] {
        app.toolbar_hidden = hidden;
        app.collapsed = collapsed;
        app.set_mouse_passthrough(true);
        assert!(app.tool == Tool::Pen);
        app.tool = Tool::Mouse;
        board_frame(&mut app, &ctx, vec![]);
        assert!(app.tool == Tool::Pen);
    }
    app.collapsed = false;
    app.mode = AppMode::Drawing;
    for size in [
        Vec2::new(320.0, 240.0),
        Vec2::new(800.0, 600.0),
        Vec2::new(1920.0, 1080.0),
    ] {
        for _ in 0..3 {
            frame(&mut app, &ctx, size, vec![], true);
        }
        assert_eq!(app.controls.len(), 4);
        assert!(app.controls[0].bottom() < app.controls[3].top());
        assert!(
            app.controls
                .iter()
                .all(|r| Rect::from_min_size(Pos2::ZERO, size).contains_rect(*r))
        );
    }
    if cfg!(windows) {
        click_text(&mut app, &ctx, "鼠标穿透");
        assert!(app.tool == Tool::Mouse);
        let pos = app.controls[3].center();
        assert!(!features::passthrough_at(
            pos,
            ctx.content_rect(),
            &app.controls
        ));
        click_text(&mut app, &ctx, "退出鼠标穿透");
        assert!(app.tool == Tool::Pen);
    }
}
