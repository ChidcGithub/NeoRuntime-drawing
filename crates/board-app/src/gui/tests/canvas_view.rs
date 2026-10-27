// 无限黑板视图：缩放（10%–500%）、平移、双指捏合跟手、按页记忆。
// 书写模式不变量：多指只写字，绝不触发缩放/平移。

fn wheel(delta: egui::Vec2, ctrl: bool) -> egui::Event {
    egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta,
        phase: egui::TouchPhase::Move,
        modifiers: if ctrl {
            egui::Modifiers::CTRL
        } else {
            egui::Modifiers::default()
        },
    }
}

#[test]
fn ctrl_wheel_zoom_is_pointer_anchored_and_clamped() {
    let mut app = app();
    let ctx = egui::Context::default();
    let size = Vec2::new(800.0, 600.0);
    frame(&mut app, &ctx, size, vec![], false);
    let cursor = Pos2::new(400.0, 300.0);
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(cursor), wheel(egui::vec2(0.0, 120.0), true)],
        false,
    );
    let zoom = app.view_zoom;
    assert!((zoom - (120.0f32 * 0.0015).exp()).abs() < 1e-4, "zoom={zoom}");
    // 指针下的板面点保持不动（锚定缩放）。
    let anchor = app.screen_to_board(cursor);
    assert!((anchor.x - 400.0).abs() < 1e-3 && (anchor.y - 300.0).abs() < 1e-3);
    // 极大滚轮量钳制到 500%。
    for _ in 0..10 {
        frame(&mut app, &ctx, size, vec![wheel(egui::vec2(0.0, 4000.0), true)], false);
    }
    assert_eq!(app.view_zoom, 5.0);
    // 缩小钳制到 10%。
    for _ in 0..10 {
        frame(&mut app, &ctx, size, vec![wheel(egui::vec2(0.0, -4000.0), true)], false);
    }
    assert_eq!(app.view_zoom, 0.1);
}

#[test]
fn plain_wheel_pans_view() {
    let mut app = app();
    let ctx = egui::Context::default();
    let size = Vec2::new(800.0, 600.0);
    frame(&mut app, &ctx, size, vec![], false);
    frame(
        &mut app,
        &ctx,
        size,
        vec![wheel(egui::vec2(0.0, -120.0), false)],
        false,
    );
    assert!((app.view_pan.y - (-120.0)).abs() < 1e-4);
    assert!((app.view_pan.x).abs() < 1e-4);
}

#[test]
fn select_two_finger_pinch_is_anchor_pinned() {
    let mut app = app();
    let ctx = egui::Context::default();
    let size = Vec2::new(800.0, 600.0);
    app.tool = Tool::Select;
    frame(&mut app, &ctx, size, vec![], false);
    // 双指落下：中点 (150,300)，距离 100。
    frame(
        &mut app,
        &ctx,
        size,
        vec![
            touch_event(1, egui::TouchPhase::Start, 100.0, 300.0),
            touch_event(2, egui::TouchPhase::Start, 200.0, 300.0),
        ],
        false,
    );
    assert!(app.pinch.is_some());
    let anchor = app.pinch.unwrap().anchor;
    assert!((anchor.x - 150.0).abs() < 1e-3 && (anchor.y - 300.0).abs() < 1e-3);
    // 张开到 2 倍距离：缩放 200%，中点下的板面点仍钉在中点下。
    frame(
        &mut app,
        &ctx,
        size,
        vec![
            touch_event(1, egui::TouchPhase::Move, 50.0, 300.0),
            touch_event(2, egui::TouchPhase::Move, 250.0, 300.0),
        ],
        false,
    );
    assert!((app.view_zoom - 2.0).abs() < 1e-4, "zoom={}", app.view_zoom);
    let pinned = app.screen_to_board(Pos2::new(150.0, 300.0));
    assert!((pinned.x - anchor.x).abs() < 1e-3 && (pinned.y - anchor.y).abs() < 1e-3);
    // 一指抬起：捏合结束。
    frame(
        &mut app,
        &ctx,
        size,
        vec![touch_event(2, egui::TouchPhase::End, 250.0, 300.0)],
        false,
    );
    assert!(app.pinch.is_none());
    assert_eq!(app.select_touches.len(), 1, "另一指仍在");
    frame(
        &mut app,
        &ctx,
        size,
        vec![touch_event(1, egui::TouchPhase::End, 50.0, 300.0)],
        false,
    );
    assert!(app.select_touches.is_empty());
}

#[test]
fn pen_mode_touches_never_pan_or_zoom() {
    let mut app = app();
    let ctx = egui::Context::default();
    let size = Vec2::new(800.0, 600.0);
    frame(&mut app, &ctx, size, vec![], false);
    // 书写模式双指张开：只写字，不缩放不平移。
    frame(
        &mut app,
        &ctx,
        size,
        vec![
            touch_event(1, egui::TouchPhase::Start, 100.0, 100.0),
            touch_event(2, egui::TouchPhase::Start, 100.0, 300.0),
        ],
        false,
    );
    frame(
        &mut app,
        &ctx,
        size,
        vec![
            touch_event(1, egui::TouchPhase::Move, 160.0, 100.0),
            touch_event(2, egui::TouchPhase::Move, 260.0, 300.0),
        ],
        false,
    );
    frame(
        &mut app,
        &ctx,
        size,
        vec![
            touch_event(1, egui::TouchPhase::End, 180.0, 100.0),
            touch_event(2, egui::TouchPhase::End, 300.0, 300.0),
        ],
        false,
    );
    assert_eq!(app.view_zoom, 1.0);
    assert_eq!(app.view_pan, egui::Vec2::ZERO);
    assert!(app.pinch.is_none());
    assert_eq!(
        app.session.document.current_page().objects.len(),
        2,
        "双指各成一笔"
    );
}

#[test]
fn select_empty_touch_drag_pans_with_inertia() {
    let mut app = app();
    let ctx = egui::Context::default();
    let size = Vec2::new(800.0, 600.0);
    app.tool = Tool::Select;
    frame(&mut app, &ctx, size, vec![], false);
    // 空白处单指按住并向左拖动：内容跟手（pan.x 增大）。
    frame(
        &mut app,
        &ctx,
        size,
        vec![touch_event(1, egui::TouchPhase::Start, 400.0, 300.0)],
        false,
    );
    assert!(app.view_drag.is_some());
    frame(
        &mut app,
        &ctx,
        size,
        vec![touch_event(1, egui::TouchPhase::Move, 360.0, 300.0)],
        false,
    );
    // 手指向左拖：内容跟随手指向左（pan.x 减小）。
    assert!(app.view_pan.x < 0.0, "pan={:?}", app.view_pan);
    // 松手带惯性：后续帧继续沿拖动方向滑行。
    frame(
        &mut app,
        &ctx,
        size,
        vec![touch_event(1, egui::TouchPhase::End, 360.0, 300.0)],
        false,
    );
    assert!(app.view_drag.is_none());
    let pan_after_release = app.view_pan;
    frame(&mut app, &ctx, size, vec![], false);
    assert!(app.view_pan.x < pan_after_release.x, "惯性滑行");
}

#[test]
fn view_state_is_remembered_per_page() {
    let mut app = app();
    let ctx = egui::Context::default();
    let size = Vec2::new(800.0, 600.0);
    frame(&mut app, &ctx, size, vec![], false);
    // add_page 会切换到新页：新页为 pages[1]。
    app.session.document.add_page().unwrap();
    frame(&mut app, &ctx, size, vec![], false);
    let cursor = Pos2::new(400.0, 300.0);
    // 新页：锚定缩放 + 平移。
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(cursor), wheel(egui::vec2(0.0, 120.0), true)],
        false,
    );
    frame(
        &mut app,
        &ctx,
        size,
        vec![wheel(egui::vec2(30.0, -40.0), false)],
        false,
    );
    let zoom_new = app.view_zoom;
    let pan_new = app.view_pan;
    assert!(zoom_new > 1.0);
    // 翻回第 1 页（index 0）：恢复默认视图。
    app.select_page(0);
    frame(&mut app, &ctx, size, vec![], false);
    assert_eq!(app.view_zoom, 1.0);
    assert_eq!(app.view_pan, egui::Vec2::ZERO);
    // 再翻回新页：恢复原视图（记住上一页位置）。
    app.select_page(1);
    frame(&mut app, &ctx, size, vec![], false);
    assert!((app.view_zoom - zoom_new).abs() < 1e-5);
    assert_eq!(app.view_pan, pan_new);
}

#[test]
fn screen_board_conversion_round_trips() {
    let mut app = app();
    app.view_zoom = 2.5;
    app.view_pan = egui::vec2(30.0, -40.0);
    for point in [
        Point { x: 0.0, y: 0.0 },
        Point { x: 12.5, y: -8.0 },
        Point { x: -100.0, y: 250.0 },
    ] {
        let screen = Pos2::new(
            point.x * app.view_zoom + app.view_pan.x,
            point.y * app.view_zoom + app.view_pan.y,
        );
        let back = app.screen_to_board(screen);
        assert!((back.x - point.x).abs() < 1e-3 && (back.y - point.y).abs() < 1e-3);
    }
}

fn five_touches(phase: egui::TouchPhase, x: f32, y: f32) -> Vec<egui::Event> {
    (0..5)
        .map(|i| touch_event(i, phase, x, y + i as f32 * 30.0))
        .collect()
}

#[test]
fn five_finger_swipe_down_minimizes_on_release() {
    let mut app = app();
    let ctx = egui::Context::default();
    let size = Vec2::new(800.0, 600.0);
    frame(&mut app, &ctx, size, five_touches(egui::TouchPhase::Start, 200.0, 150.0), false);
    // 快速下滑 40px：越过死区（速度样本需一帧，锁向在其后一帧生效）。
    frame(
        &mut app,
        &ctx,
        size,
        (0..5)
            .map(|i| {
                touch_event(i, egui::TouchPhase::Move, 200.0, 180.0 + i as f32 * 30.0)
            })
            .collect(),
        false,
    );
    // 继续到阈值：弹跳提示松手执行。
    frame(
        &mut app,
        &ctx,
        size,
        (0..5)
            .map(|i| {
                touch_event(i, egui::TouchPhase::Move, 200.0, 270.0 + i as f32 * 30.0)
            })
            .collect(),
        false,
    );
    let swipe = app.swipe.unwrap();
    assert!(swipe.progress >= 1.0 && swipe.popped);
    // 全部抬手：执行最小化。
    let output = frame(
        &mut app,
        &ctx,
        size,
        five_touches(egui::TouchPhase::End, 200.0, 270.0),
        false,
    );
    assert!(
        output
            .viewport_output
            .values()
            .any(|viewport| {
                viewport
                    .commands
                    .iter()
                    .any(|command| {
                        matches!(command, egui::ViewportCommand::Minimized(true))
                    })
            }),
        "下滑释放最小化"
    );
}

#[test]
fn five_finger_swipe_right_flips_to_next_page() {
    let mut app = app();
    let ctx = egui::Context::default();
    let size = Vec2::new(800.0, 600.0);
    app.session.document.add_page().unwrap();
    app.session.document.set_current_page(0).unwrap();
    frame(&mut app, &ctx, size, five_touches(egui::TouchPhase::Start, 300.0, 200.0), false);
    frame(
        &mut app,
        &ctx,
        size,
        (0..5)
            .map(|i| {
                touch_event(i, egui::TouchPhase::Move, 340.0, 200.0 + i as f32 * 30.0)
            })
            .collect(),
        false,
    );
    frame(
        &mut app,
        &ctx,
        size,
        (0..5)
            .map(|i| {
                touch_event(i, egui::TouchPhase::Move, 430.0, 200.0 + i as f32 * 30.0)
            })
            .collect(),
        false,
    );
    frame(&mut app, &ctx, size, five_touches(egui::TouchPhase::End, 430.0, 200.0), false);
    assert_eq!(app.session.document.current_page, 1, "右滑翻到下一页");
}

#[test]
fn five_finger_swipe_return_cancels() {
    let mut app = app();
    let ctx = egui::Context::default();
    let size = Vec2::new(800.0, 600.0);
    frame(&mut app, &ctx, size, five_touches(egui::TouchPhase::Start, 200.0, 150.0), false);
    frame(
        &mut app,
        &ctx,
        size,
        (0..5)
            .map(|i| {
                touch_event(i, egui::TouchPhase::Move, 200.0, 180.0 + i as f32 * 30.0)
            })
            .collect(),
        false,
    );
    frame(
        &mut app,
        &ctx,
        size,
        (0..5)
            .map(|i| {
                touch_event(i, egui::TouchPhase::Move, 200.0, 270.0 + i as f32 * 30.0)
            })
            .collect(),
        false,
    );
    let swipe = app.swipe.expect("快速下滑锁向");
    assert!(swipe.progress >= 1.0 && swipe.popped);
    // 手返回：进度回落退出弹跳态，松手不执行。
    frame(
        &mut app,
        &ctx,
        size,
        (0..5)
            .map(|i| {
                touch_event(i, egui::TouchPhase::Move, 200.0, 170.0 + i as f32 * 30.0)
            })
            .collect(),
        false,
    );
    let swipe = app.swipe.unwrap();
    assert!(swipe.progress < 0.85 && !swipe.popped);
    let output = frame(
        &mut app,
        &ctx,
        size,
        five_touches(egui::TouchPhase::End, 200.0, 170.0),
        false,
    );
    assert!(
        !output
            .viewport_output
            .values()
            .any(|viewport| {
                viewport
                    .commands
                    .iter()
                    .any(|command| {
                        matches!(command, egui::ViewportCommand::Minimized(true))
                    })
            }),
        "返回后松手不执行"
    );
    assert_eq!(app.session.document.current_page, 0);
}

#[test]
fn slow_five_finger_moves_never_lock_swipe() {
    let mut app = app();
    let ctx = egui::Context::default();
    let size = Vec2::new(800.0, 600.0);
    frame(&mut app, &ctx, size, five_touches(egui::TouchPhase::Start, 200.0, 100.0), false);
    // 缓慢下移（每帧 1px）：书写级速度，绝不锁向，笔画保留。
    for step in 1..=40 {
        frame(
            &mut app,
            &ctx,
            size,
            (0..5)
                .map(|i| {
                    touch_event(
                        i,
                        egui::TouchPhase::Move,
                        200.0,
                        100.0 + i as f32 * 30.0 + step as f32,
                    )
                })
                .collect(),
            false,
        );
    }
    assert!(app.swipe.is_none(), "缓慢移动不触发手势");
    frame(&mut app, &ctx, size, five_touches(egui::TouchPhase::End, 200.0, 140.0), false);
    assert_eq!(
        app.session.document.current_page().objects.len(),
        5,
        "五指笔画全部保留"
    );
    assert!(app.view_zoom == 1.0 && app.view_pan == egui::Vec2::ZERO);
}

#[test]
fn zoom_hud_fades_in_holds_one_second_then_fades_out() {
    let mut app = app();
    let ctx = egui::Context::default();
    let size = Vec2::new(800.0, 600.0);
    let cursor = Pos2::new(400.0, 300.0);
    let frame_at = |app: &mut BoardApp, ctx: &egui::Context, events: Vec<egui::Event>, time: f64| {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            events,
            time: Some(time),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.canvas(ui));
        output.textures_delta.clear();
        output
    };
    frame_at(&mut app, &ctx, vec![], 0.0);
    assert!(app.zoom_hud.is_none(), "未缩放不显示");
    // 缩放：提示淡入。
    frame_at(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(cursor), wheel(egui::vec2(0.0, 120.0), true)],
        0.10,
    );
    assert!(app.zoom_hud.is_some(), "缩放后显示比例提示");
    // 持续缩放：提示保持并刷新计时。
    frame_at(&mut app, &ctx, vec![wheel(egui::vec2(0.0, 60.0), true)], 0.50);
    assert!(app.zoom_hud.is_some());
    // 停止后 1 秒内：仍在。
    frame_at(&mut app, &ctx, vec![], 1.40);
    assert!(app.zoom_hud.is_some(), "淡出前等待 1 秒");
    // 停止后超过 1 秒 + 淡出完成：隐藏。
    frame_at(&mut app, &ctx, vec![], 1.90);
    assert!(app.zoom_hud.is_none(), "淡出完成后隐藏");
}

#[test]
fn drawing_mode_stays_one_to_one_without_infinite_canvas() {
    // 画板是透明标注浮层：无无限黑板，视图恒为 1:1（板面=屏幕）。
    let mut session = Session::new(AppMode::Drawing.kind());
    session.handle(
        Request::new(
            "neo:setup",
            "configure",
            serde_json::json!({"classroom_safe": true, "desktop_capture_allowed": false, "agent_allowed": false}),
        )
        .unwrap(),
    );
    let mut app = BoardApp::new(AppMode::Drawing, session, false, None, Default::default());
    let ctx = egui::Context::default();
    let size = Vec2::new(800.0, 600.0);
    let cursor = Pos2::new(400.0, 300.0);
    frame(&mut app, &ctx, size, vec![], false);
    // Ctrl+滚轮：不缩放。
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(cursor), wheel(egui::vec2(0.0, 4000.0), true)],
        false,
    );
    assert_eq!(app.view_zoom, 1.0, "画板不缩放");
    // 滚轮平移：不平移。
    frame(&mut app, &ctx, size, vec![wheel(egui::vec2(0.0, -400.0), false)], false);
    assert_eq!(app.view_pan, egui::Vec2::ZERO, "画板不平移");
    // 中键拖动：不平移。
    frame(
        &mut app,
        &ctx,
        size,
        vec![pointer(cursor, true)],
        false,
    );
    assert_eq!(app.view_pan, egui::Vec2::ZERO);
    // 选择模式双指捏合：不捏合。
    app.tool = Tool::Select;
    frame(&mut app, &ctx, size, vec![pointer(cursor, false)], false);
    frame(
        &mut app,
        &ctx,
        size,
        vec![
            touch_event(1, egui::TouchPhase::Start, 100.0, 300.0),
            touch_event(2, egui::TouchPhase::Start, 200.0, 300.0),
        ],
        false,
    );
    frame(
        &mut app,
        &ctx,
        size,
        vec![
            touch_event(1, egui::TouchPhase::Move, 40.0, 300.0),
            touch_event(2, egui::TouchPhase::Move, 260.0, 300.0),
        ],
        false,
    );
    assert!(app.pinch.is_none(), "画板无捏合");
    assert_eq!(app.view_zoom, 1.0);
    assert_eq!(app.view_pan, egui::Vec2::ZERO);
}

#[test]
fn touch_select_drag_commits_the_move() {
    // 触摸选择拖动：预览跟手，抬手必须提交移动（touch_gesture 归属标记）。
    let line = BoardObject {
        id: "l".into(),
        kind: ObjectKind::Shape {
            shape: ShapeKind::Line,
            points: vec![Point { x: 100.0, y: 100.0 }, Point { x: 200.0, y: 100.0 }],
            style: Default::default(),
        },
    };
    let (mut app, ctx) = plot_drag_app(vec![line]);
    let size = Vec2::new(800.0, 600.0);
    frame(&mut app, &ctx, size, vec![], false);
    // 按在线中点 (150,100)，拖到 (250,180)，抬手。
    frame(
        &mut app,
        &ctx,
        size,
        vec![touch_event(1, egui::TouchPhase::Start, 150.0, 100.0)],
        false,
    );
    assert!(app.gesture.is_some(), "触摸选中对象应创建拖动手势");
    assert_eq!(app.touch_gesture, Some(egui::TouchId(1)), "归属触点应记录");
    frame(
        &mut app,
        &ctx,
        size,
        vec![touch_event(1, egui::TouchPhase::Move, 250.0, 180.0)],
        false,
    );
    frame(
        &mut app,
        &ctx,
        size,
        vec![touch_event(1, egui::TouchPhase::End, 250.0, 180.0)],
        false,
    );
    assert!(app.gesture.is_none(), "抬手后手势应结束");
    let points = match &app.session.document.current_page().objects[0].kind {
        ObjectKind::Shape { points, .. } => points.clone(),
        other => panic!("unexpected object: {other:?}"),
    };
    assert_eq!(
        points,
        vec![Point { x: 200.0, y: 180.0 }, Point { x: 300.0, y: 180.0 }],
        "触摸拖动应提交 (100,80) 的位移"
    );
}

#[test]
fn selected_coordinate_system_offers_three_d_toggle() {
    // 选中坐标系：附近出现三维/二维切换按钮（登记为可点击控件）。
    let (mut app, ctx) = plot_drag_app(vec![BoardObject {
        id: "cs".into(),
        kind: ObjectKind::CoordinateSystem {
            origin: Point { x: 200.0, y: 200.0 },
            scale: 20.0,
            axis_width: None,
            three_d: false,
        },
    }]);
    let size = Vec2::new(800.0, 600.0);
    // 切换按钮在 plot_controls（选中对象浮层），需与 canvas 同帧调用。
    let ui_frame = |app: &mut BoardApp, ctx: &egui::Context| {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            app.plot_controls(ctx);
            app.canvas(ui);
        });
        output.textures_delta.clear();
        output
    };
    ui_frame(&mut app, &ctx);
    app.selected = Some("cs".into());
    ui_frame(&mut app, &ctx);
    let near_button =
        egui::Rect::from_center_size(Pos2::new(200.0, 170.0), Vec2::new(160.0, 60.0));
    assert!(
        app.controls.iter().any(|r| r.intersects(near_button)),
        "选中坐标系应出现切换按钮：controls={:?}",
        app.controls
    );
    // 三维状态下提供切回二维。
    let mut toggled = app.session.document.current_page().objects[0].clone();
    if let ObjectKind::CoordinateSystem { three_d, .. } = &mut toggled.kind {
        *three_d = true;
    }
    app.apply(vec![Operation::Update { object: toggled }]);
    ui_frame(&mut app, &ctx);
    assert!(
        app.controls.iter().any(|r| r.intersects(near_button)),
        "翻转后切换按钮仍在"
    );
    assert!(matches!(
        app.session.document.current_page().objects[0].kind,
        ObjectKind::CoordinateSystem { three_d: true, .. }
    ));
}

// 工具栏二级菜单展开时不阻塞画布：点击工具栏以外时收起菜单，且这一次
// 按下照常落笔（菜单本身区域仍不响应画布）。
#[test]
fn toolbar_menu_does_not_block_canvas_and_closes_on_outside_press() {
    let mut app = app();
    let ctx = egui::Context::default();
    let size = Vec2::new(800.0, 600.0);
    frame(&mut app, &ctx, size, vec![], false);
    // 模拟二级菜单已展开。
    egui::Popup::open_id(&ctx, egui::Id::new("toolbar_secondary"));
    assert!(egui::Popup::is_any_open(&ctx));
    // 菜单展开时在画布起笔：不应被阻塞。
    let start = Pos2::new(320.0, 300.0);
    frame(&mut app, &ctx, size, vec![pointer(start, true)], false);
    assert!(app.gesture.is_some(), "二级菜单展开时仍应能起笔");
    // 同一次按下收起菜单（点击工具栏以外）。
    assert!(!egui::Popup::is_any_open(&ctx), "点击工具栏以外应收起二级菜单");
    // 继续拖动并抬手：笔画落盘。
    frame(
        &mut app,
        &ctx,
        size,
        vec![pointer(Pos2::new(380.0, 320.0), true)],
        false,
    );
    frame(
        &mut app,
        &ctx,
        size,
        vec![pointer(Pos2::new(380.0, 320.0), false)],
        false,
    );
    assert_eq!(
        app.session.document.current_page().objects.len(),
        1,
        "菜单收起后笔画应落盘"
    );
    // 菜单自身区域（登记为控件）按下时不落笔。
    app.controls
        .push(Rect::from_min_size(Pos2::new(600.0, 100.0), Vec2::new(120.0, 80.0)));
    frame(
        &mut app,
        &ctx,
        size,
        vec![pointer(Pos2::new(650.0, 130.0), true)],
        false,
    );
    assert!(app.gesture.is_none(), "菜单区域内按下不应起笔");
    app.controls.clear();
}

// 坐标系一键绘图：轴线取当前笔宽×0.8（略细于笔），最细笔也被钳制到
// 合法范围，不会因为 0.08 被核心校验拒绝而创建失败。
#[test]
fn coordinate_system_axes_follow_pen_width_even_at_thinnest_pen() {
    let mut app = app();
    let ctx = egui::Context::default();
    let size = Vec2::new(800.0, 600.0);
    let draw = |app: &mut BoardApp, ctx: &egui::Context, from: Pos2, to: Pos2| {
        frame(app, ctx, size, vec![], false);
        frame(app, ctx, size, vec![pointer(from, true)], false);
        frame(app, ctx, size, vec![pointer(to, true)], false);
        frame(app, ctx, size, vec![pointer(to, false)], false);
    };
    app.tool = Tool::Coordinates;
    app.style.width = 4.0;
    draw(&mut app, &ctx, Pos2::new(300.0, 300.0), Pos2::new(420.0, 300.0));
    let objects = &app.session.document.current_page().objects;
    assert_eq!(objects.len(), 1, "坐标系应创建成功");
    match &objects[0].kind {
        ObjectKind::CoordinateSystem {
            axis_width, three_d, ..
        } => {
            assert!(!*three_d, "默认二维");
            assert!(
                (axis_width.expect("轴线宽度应记住笔宽") - 3.2).abs() < 1e-4,
                "轴线 = 笔宽×0.8：{axis_width:?}"
            );
        }
        other => panic!("期望坐标系：{other:?}"),
    }
    // 最细笔（0.1×0.8=0.08 会被核心拒绝）钳制到下限。
    app.tool = Tool::Coordinates;
    app.style.width = 0.1;
    draw(&mut app, &ctx, Pos2::new(300.0, 450.0), Pos2::new(420.0, 450.0));
    let objects = &app.session.document.current_page().objects;
    assert_eq!(objects.len(), 2, "最细笔也应创建成功");
    match &objects[1].kind {
        ObjectKind::CoordinateSystem { axis_width, .. } => assert!(
            (axis_width.expect("轴线宽度") - board_core::MIN_BRUSH_WIDTH).abs() < 1e-6,
            "钳制到下限于：{axis_width:?}"
        ),
        other => panic!("期望坐标系：{other:?}"),
    }
}
