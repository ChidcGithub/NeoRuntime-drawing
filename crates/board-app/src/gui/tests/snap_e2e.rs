// 吸附端到端测试：线段工具 × 鼠标/触屏 × 端点/交点/衰减，走真实输入管线
// （frame 的指针事件 / multi_touch 的 Touch 事件 → gui.rs 手势 → preview_for 吸附）。
//
// v0.1.1-r2：Tool::Shape/Eraser/Coordinates 的触摸落笔已创建手势（与指针路径
// 同构），触屏吸附与鼠标一致；交点吸附语义为「线身经过交点则整条线吸附经过
// 之（snap_segment_through_intersections），端点吸附保留为候选回退」。

fn snap_line(id: &str, a: Point, b: Point) -> BoardObject {
    BoardObject {
        id: id.into(),
        kind: ObjectKind::Shape {
            shape: ShapeKind::Line,
            points: vec![a, b],
            style: Style::default(),
        },
    }
}

fn snap_app(objects: Vec<BoardObject>) -> (BoardApp, egui::Context) {
    let (mut app, ctx) = plot_drag_app(objects);
    app.tool = Tool::Shape;
    app.shape = ShapeKind::Line;
    (app, ctx)
}

/// 预置对象 id 以 "preset-" 开头；提交后按该前缀过滤出「新画」的对象。
fn new_objects(app: &BoardApp) -> Vec<BoardObject> {
    app.session
        .document
        .current_page()
        .objects
        .iter()
        .filter(|object| !object.id.starts_with("preset-"))
        .cloned()
        .collect()
}

/// 与 frame 相同，但显式设定 egui 时钟（RawInput.time），用于控制拖动时长。
/// egui 0.36：`InputState.time = raw.time.unwrap_or(...)`，显式 time 精确生效。
fn timed_frame(
    app: &mut BoardApp,
    ctx: &egui::Context,
    size: Vec2,
    events: Vec<egui::Event>,
    time: f64,
) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
        events,
        time: Some(time),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.canvas(ui));
    output.textures_delta.clear();
    output
}

fn drop_offset(point: Point, drop: Point) -> f64 {
    ((point.x - drop.x).hypot(point.y - drop.y)) as f64
}

/// a) 鼠标线段工具端点吸附：落点距端点约 8.2px ≤ 初始吸附半径 12。
#[test]
fn snap_e2e_mouse_line_endpoint_snap() {
    let (mut app, ctx) = snap_app(vec![snap_line(
        "preset-x",
        Point { x: 0.0, y: 0.0 },
        Point { x: 100.0, y: 0.0 },
    )]);
    let size = Vec2::new(800.0, 600.0);
    frame(&mut app, &ctx, size, vec![], false);
    let start = Pos2::new(98.0, 8.0); // 距端点 (100,0) ≈ 8.2px
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
        false,
    );
    assert!(app.gesture.is_some(), "鼠标按下必须创建手势");
    let end = Pos2::new(200.0, 60.0);
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(end)],
        false,
    );
    let preview = app.preview().expect("拖动中应有形状预览");
    assert_eq!(
        shape_points(&preview)[0],
        Point { x: 100.0, y: 0.0 },
        "预览起点应吸附到端点 (100,0)（端点优先于更近的边投影 (98,0)）"
    );
    release(&mut app, &ctx, end);
    let drawn = new_objects(&app);
    assert_eq!(drawn.len(), 1, "抬手应提交恰好 1 条新线段");
    assert_eq!(
        shape_points(&drawn[0])[0],
        Point { x: 100.0, y: 0.0 },
        "提交对象起点应保持在吸附端点 (100,0)"
    );
    assert_ne!(
        shape_points(&drawn[0])[1],
        Point { x: 100.0, y: 0.0 },
        "末端不应被吸回起点"
    );
    let drift = drop_offset(shape_points(&drawn[0])[1], Point { x: 200.0, y: 60.0 });
    assert!(
        drift < 5.0,
        "末端应基本落在放手处（仅 30° 角度吸附微调），实际偏移 {drift}px"
    );
}

/// b) 触摸线段工具端点吸附：与 a) 相同场景，仅事件换成 Touch。
#[test]
fn snap_e2e_touch_line_endpoint_snap() {
    let (mut app, ctx) = snap_app(vec![snap_line(
        "preset-x",
        Point { x: 0.0, y: 0.0 },
        Point { x: 100.0, y: 0.0 },
    )]);
    let size = Vec2::new(800.0, 600.0);
    frame(&mut app, &ctx, size, vec![], false);
    frame(
        &mut app,
        &ctx,
        size,
        vec![touch_event(1, egui::TouchPhase::Start, 98.0, 8.0)],
        false,
    );
    assert!(app.touch_writing, "触摸应进入触控书写模式");
    frame(
        &mut app,
        &ctx,
        size,
        vec![touch_event(1, egui::TouchPhase::Move, 200.0, 60.0)],
        false,
    );
    assert!(
        app.preview().is_some(),
        "触屏拖动中应有形状预览；当前 Tool::Shape 触摸落笔不创建手势\
         （gui.rs TouchPhase::Start 仅处理 Pen/Select 两个工具），吸附无从生效"
    );
    frame(
        &mut app,
        &ctx,
        size,
        vec![touch_event(1, egui::TouchPhase::End, 200.0, 60.0)],
        false,
    );
    let drawn = new_objects(&app);
    assert_eq!(
        drawn.len(),
        1,
        "触屏线段工具应提交 1 条线段；当前 TouchPhase::Start/Move/End \
         对 Tool::Shape 既无手势也无提交路径"
    );
    assert_eq!(
        shape_points(&drawn[0])[0],
        Point { x: 100.0, y: 0.0 },
        "触屏落笔端点吸附应与鼠标一致"
    );
}

/// c) 鼠标线段工具交点吸附：末端落在交点 (50,0) 附近 4.5px，交点应压过
/// 更近的边投影（(50,4) 距 2px、(52,0) 距 4px）。
#[test]
fn snap_e2e_mouse_line_intersection_snap() {
    let (mut app, ctx) = snap_app(vec![
        snap_line(
            "preset-x",
            Point { x: 0.0, y: 0.0 },
            Point { x: 100.0, y: 0.0 },
        ),
        snap_line(
            "preset-y",
            Point { x: 50.0, y: -40.0 },
            Point { x: 50.0, y: 40.0 },
        ),
    ]);
    let size = Vec2::new(800.0, 600.0);
    frame(&mut app, &ctx, size, vec![], false);
    // 起点远离一切候选；画线方向 ≈217.8°，避开 15° 倍数的角度吸附带（±6°）。
    let start = Pos2::new(240.0, 150.0);
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
        false,
    );
    let end = Pos2::new(52.0, 4.0); // 距交点 (50,0) ≈ 4.5px
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(end)],
        false,
    );
    let preview = app.preview().expect("拖动中应有形状预览");
    assert_eq!(
        shape_points(&preview)[1],
        Point { x: 50.0, y: 0.0 },
        "预览末端应吸附到交点 (50,0)（交点分级压过边投影）"
    );
    release(&mut app, &ctx, end);
    let drawn = new_objects(&app);
    assert_eq!(drawn.len(), 1, "抬手应提交恰好 1 条新线段");
    assert_eq!(
        shape_points(&drawn[0])[1],
        Point { x: 50.0, y: 0.0 },
        "提交末端应为交点 (50,0)"
    );
}

/// c-touch) 交点吸附的触摸对照：与 c) 相同场景改用 Touch 事件。
#[test]
fn snap_e2e_touch_line_intersection_snap() {
    let (mut app, ctx) = snap_app(vec![
        snap_line(
            "preset-x",
            Point { x: 0.0, y: 0.0 },
            Point { x: 100.0, y: 0.0 },
        ),
        snap_line(
            "preset-y",
            Point { x: 50.0, y: -40.0 },
            Point { x: 50.0, y: 40.0 },
        ),
    ]);
    let size = Vec2::new(800.0, 600.0);
    frame(&mut app, &ctx, size, vec![], false);
    frame(
        &mut app,
        &ctx,
        size,
        vec![touch_event(1, egui::TouchPhase::Start, 240.0, 150.0)],
        false,
    );
    frame(
        &mut app,
        &ctx,
        size,
        vec![touch_event(1, egui::TouchPhase::Move, 52.0, 4.0)],
        false,
    );
    frame(
        &mut app,
        &ctx,
        size,
        vec![touch_event(1, egui::TouchPhase::End, 52.0, 4.0)],
        false,
    );
    let drawn = new_objects(&app);
    assert_eq!(
        drawn.len(),
        1,
        "触屏交点吸附应提交 1 条线段；当前 Tool::Shape 触摸路径无手势无提交"
    );
    assert_eq!(
        shape_points(&drawn[0])[1],
        Point { x: 50.0, y: 0.0 },
        "触屏末端应与鼠标一致吸附到交点 (50,0)"
    );
}

/// d) 吸附半径随拖动时长衰减：距端点 11px 的落点，半径未衰减（≈11.94）时
/// 吸附，半径衰减到 ≈9.8（拖动 0.4s）后不再吸附。时间用 RawInput.time
/// 显式步进，不依赖真实墙钟。断言停在预览层（绘制吸附）——提交阶段
/// commit_line 用固定 12px 的 connection_hit 建立连接并把端点传播到锚点，
/// 会把 11px 落点重新拉回端点，衰减对提交不可见（见 eprintln 观察）。
#[test]
fn snap_e2e_snap_radius_decay() {
    // 单元锚点：半径单调不增；t=0 为上限 12，足够长后触底下限 2。
    assert!((features::drag_snap_radius(0.0) - features::SNAP_RADIUS_MAX).abs() < 1e-4);
    assert!(features::drag_snap_radius(0.1) < features::drag_snap_radius(0.0));
    assert!(features::drag_snap_radius(2.0) < features::drag_snap_radius(0.1));
    assert_eq!(features::drag_snap_radius(60.0), features::SNAP_RADIUS_MIN);

    let size = Vec2::new(800.0, 600.0);
    // 落点距预置端点 (100,0) 恰 11px，且本身在线身上（边命中即自身，无位移）。
    let press = Pos2::new(89.0, 0.0);
    let end = Pos2::new(250.0, 80.0);

    // 快速拖动：按下 t=0，落点 t=0.01 → 半径 12·e^(−0.005) ≈ 11.94 ≥ 11 → 吸附。
    let (mut app, ctx) = snap_app(vec![snap_line(
        "preset-x",
        Point { x: 0.0, y: 0.0 },
        Point { x: 100.0, y: 0.0 },
    )]);
    timed_frame(&mut app, &ctx, size, vec![], 0.0);
    timed_frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(press), pointer(press, true)],
        0.0,
    );
    timed_frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(end)],
        0.01,
    );
    let quick = app.preview().expect("拖动中应有形状预览");
    assert_eq!(
        shape_points(&quick)[0],
        Point { x: 100.0, y: 0.0 },
        "半径 ≈11.94 ≥ 11：快速落点应吸附端点 (100,0)"
    );
    release(&mut app, &ctx, end);
    let quick_drawn = new_objects(&app);
    assert_eq!(quick_drawn.len(), 1);
    assert_eq!(
        shape_points(&quick_drawn[0])[0],
        Point { x: 100.0, y: 0.0 },
        "快速拖动提交后起点应保持在 (100,0)"
    );

    // 慢速拖动：多次小步把拖动时长拉到 0.4s → 半径 12·e^(−0.2) ≈ 9.82 < 11。
    let (mut app, ctx) = snap_app(vec![snap_line(
        "preset-x",
        Point { x: 0.0, y: 0.0 },
        Point { x: 100.0, y: 0.0 },
    )]);
    timed_frame(&mut app, &ctx, size, vec![], 0.0);
    timed_frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(press), pointer(press, true)],
        0.0,
    );
    for (index, time) in [0.1f64, 0.2, 0.3, 0.4].into_iter().enumerate() {
        let step = Pos2::new(130.0 + 40.0 * index as f32, 20.0 + 20.0 * index as f32);
        timed_frame(
            &mut app,
            &ctx,
            size,
            vec![egui::Event::PointerMoved(step)],
            time,
        );
    }
    let slow = app.preview().expect("拖动中应有形状预览");
    assert_eq!(
        shape_points(&slow)[0],
        Point { x: 89.0, y: 0.0 },
        "半径 12·e^(−0.4/2) ≈ 9.82 < 11：衰减后 11px 落点不应再吸附端点"
    );
    release(&mut app, &ctx, end);
    let slow_drawn = new_objects(&app);
    assert_eq!(slow_drawn.len(), 1);
    eprintln!(
        "衰减观察（非断言）：慢速拖动提交后起点 = {:?}；commit_line 用固定 12px \
         connection_hit 连接端点并传播到锚点，会把 11px 落点重新拉回 (100,0)，\
         衰减只作用于绘制吸附，不作用于提交连接",
        shape_points(&slow_drawn[0])[0]
    );
}

/// e) 特定度数吸附随拖动时长衰减：30° 方向画线，垂直偏差 6px（约 1.72°）。
/// 早期容差 ≈5.85° 吸附到 30° 射线上；拖动 3s 后容差 ≈1.34° 不再吸附。
#[test]
fn snap_e2e_angle_snap_decays_with_drag_time() {
    let size = Vec2::new(800.0, 600.0);
    let start = Pos2::new(300.0, 300.0);
    // 30° 方向、线长 200 的理想终点，再叠加垂直偏差 6px。
    let ray_end = Pos2::new(473.20508, 400.0);
    let raw_end = Pos2::new(470.205, 405.196);
    // 早期：按下 t=0，落点 t=0.05 -> 容差 ≈5.85°，偏差 1.72° 被吸附。
    let (mut app, ctx) = snap_app(vec![]);
    timed_frame(&mut app, &ctx, size, vec![], 0.0);
    timed_frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
        0.0,
    );
    timed_frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(raw_end)],
        0.05,
    );
    let preview = app.preview().expect("拖动中应有预览");
    let snapped = shape_points(&preview)[1];
    let on_ray = Pos2::new(snapped.x, snapped.y);
    assert!(
        on_ray.distance(ray_end) < 0.5,
        "早期应吸附到 30° 射线：{on_ray:?} vs {ray_end:?}"
    );
    // 后期：同一几何，拖动时长拉到 3s -> 容差 ≈1.34°，不再吸附。
    let (mut app, ctx) = snap_app(vec![]);
    timed_frame(&mut app, &ctx, size, vec![], 0.0);
    timed_frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
        0.0,
    );
    for (step, time) in [(1.0, 0.5), (2.0, 1.0), (3.0, 1.5), (4.0, 2.0), (5.0, 2.5)] {
        timed_frame(
            &mut app,
            &ctx,
            size,
            vec![egui::Event::PointerMoved(Pos2::new(
                start.x + step,
                start.y,
            ))],
            time,
        );
    }
    timed_frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(raw_end)],
        3.0,
    );
    let preview = app.preview().expect("拖动中应有预览");
    let end = shape_points(&preview)[1];
    let deviation = Pos2::new(end.x, end.y).distance(ray_end);
    assert!(
        deviation > 5.0,
        "长拖后角度吸附应失效：末端 {end:?} 偏离射线仅 {deviation}px"
    );
}
