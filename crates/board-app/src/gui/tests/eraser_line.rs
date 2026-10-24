// 橡皮擦对 Shape::Line 的行为：必须按线段几何精确命中并切割（与笔迹一致），
// 不能按包围盒整体删除。覆盖 zoom=1 与 zoom=2（含平移）下的指针与触控路径。
// 注意：首帧会按 view_page_id 归位视图（gui.rs canvas 开头），因此必须先跑
// 一帧预热，再设置 view_zoom/view_pan，否则赋值会被重置回 (1.0, ZERO)。

fn eraser_line_app() -> (BoardApp, egui::Context) {
    let mut app = app();
    app.tool = Tool::Eraser;
    app.session.document.pages[0].objects = vec![BoardObject {
        id: "line".into(),
        kind: ObjectKind::Shape {
            shape: ShapeKind::Line,
            // 斜线：包围盒远大于线段本身，bbox 误删的典型场景。
            points: vec![Point { x: 100.0, y: 100.0 }, Point { x: 500.0, y: 400.0 }],
            style: Style::default(),
        },
    }];
    app.session.history = board_core::History::new(&app.session.document);
    (app, egui::Context::default())
}

fn eraser_line_view(app: &mut BoardApp, ctx: &egui::Context, zoom: f32, pan: egui::Vec2) {
    let size = Vec2::new(1000.0, 700.0);
    frame(app, ctx, size, vec![], false);
    assert_eq!(app.view_zoom, 1.0);
    app.view_zoom = zoom;
    app.view_pan = pan;
}

fn eraser_screen(app: &BoardApp, board: (f32, f32)) -> Pos2 {
    Pos2::new(
        board.0 * app.view_zoom + app.view_pan.x,
        board.1 * app.view_zoom + app.view_pan.y,
    )
}

fn line_pieces(document: &board_core::Document) -> Vec<&BoardObject> {
    document
        .current_page()
        .objects
        .iter()
        .filter(|object| {
            matches!(
                object.kind,
                ObjectKind::Shape {
                    shape: ShapeKind::Line,
                    ..
                }
            )
        })
        .collect()
}

fn assert_point(actual: Point, expected: (f32, f32)) {
    assert!(
        (actual.x - expected.0).abs() < 1e-2 && (actual.y - expected.1).abs() < 1e-2,
        "point ({}, {}) != ({}, {})",
        actual.x,
        actual.y,
        expected.0,
        expected.1
    );
}

// 橡皮圈落在斜线包围盒内、但离线段 170 板面单位处：不应擦除任何内容。
fn eraser_miss_inside_bbox(zoom: f32, pan: egui::Vec2) {
    let (mut app, ctx) = eraser_line_app();
    eraser_line_view(&mut app, &ctx, zoom, pan);
    let before = app.session.document.clone();
    let start = eraser_screen(&app, (450.0, 150.0));
    let end = eraser_screen(&app, (460.0, 160.0));
    let size = Vec2::new(1000.0, 700.0);
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
        false,
    );
    assert_eq!(app.screen_to_board(start), Point { x: 450.0, y: 150.0 });
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(end)],
        false,
    );
    // 预览阶段：手势点距线段 170 > 橡皮半径，包围盒命中不算数。
    let preview = erased_document(&app);
    assert_eq!(preview.current_page().objects.len(), 1);
    assert_eq!(preview.current_page().objects[0].id, "line");
    // 提交阶段：松手后文档不变，不产生删除。
    frame(&mut app, &ctx, size, vec![pointer(end, false)], false);
    assert!(app.gesture.is_none());
    assert_eq!(app.session.document, before);
    assert_eq!(app.session.document.revision, before.revision);
}

#[test]
fn eraser_miss_inside_line_bbox_zoom1_does_not_delete() {
    eraser_miss_inside_bbox(1.0, egui::Vec2::ZERO);
}

#[test]
fn eraser_miss_inside_line_bbox_zoom2_with_pan_does_not_delete() {
    eraser_miss_inside_bbox(2.0, egui::vec2(50.0, -30.0));
}

// 橡皮圈从线段中点 (300,250) 划过：线段应被切成两段 Line，而不是整条消失。
fn eraser_cuts_line_at_midpoint(zoom: f32, pan: egui::Vec2) {
    let (mut app, ctx) = eraser_line_app();
    eraser_line_view(&mut app, &ctx, zoom, pan);
    let before = app.session.document.clone();
    let size = Vec2::new(1000.0, 700.0);
    let start = eraser_screen(&app, (300.0, 250.0));
    let end = eraser_screen(&app, (308.0, 256.0));
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
        false,
    );
    assert_eq!(app.screen_to_board(start), Point { x: 300.0, y: 250.0 });
    // 预览与提交一致：落笔点已在线上，预览文档应切出两段，真实文档未动。
    let preview_pieces = line_pieces(erased_document(&app));
    assert_eq!(preview_pieces.len(), 2, "预览应切出两段");
    assert_eq!(app.session.document, before);
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::PointerMoved(end)],
        false,
    );
    frame(&mut app, &ctx, size, vec![pointer(end, false)], false);
    assert!(app.gesture.is_none());
    assert_eq!(app.session.document.revision, before.revision + 1);
    let pieces = line_pieces(&app.session.document);
    assert_eq!(pieces.len(), 2, "整条线不应消失，应切成两段");
    let first = pieces[0];
    let second = pieces[1];
    let ObjectKind::Shape { points: a, .. } = &first.kind else {
        panic!("shape expected")
    };
    let ObjectKind::Shape { points: b, .. } = &second.kind else {
        panic!("shape expected")
    };
    assert_eq!(a.len(), 2);
    assert_eq!(b.len(), 2);
    // 端点保留：两段分别继承原线段两端。
    assert_point(a[0], (100.0, 100.0));
    assert_point(b[1], (500.0, 400.0));
    // 切口在橡皮圈处：两段相对端点之间存在缺口，且位于圈中心附近。
    let gap = (a[1].x - b[0].x).hypot(a[1].y - b[0].y);
    assert!(gap > 20.0, "切口缺口过小: {gap}");
    let cut_mid = Point {
        x: (a[1].x + b[0].x) / 2.0,
        y: (a[1].y + b[0].y) / 2.0,
    };
    assert!(
        (cut_mid.x - 300.0).abs() < 30.0 && (cut_mid.y - 250.0).abs() < 30.0,
        "切口偏离橡皮圈位置: ({}, {})",
        cut_mid.x,
        cut_mid.y
    );
    // 单次撤销恢复原线段。
    app.undo(false);
    assert_eq!(app.session.document.pages, before.pages);
}

#[test]
fn eraser_cuts_line_at_midpoint_zoom1() {
    eraser_cuts_line_at_midpoint(1.0, egui::Vec2::ZERO);
}

#[test]
fn eraser_cuts_line_at_midpoint_zoom2_with_pan() {
    eraser_cuts_line_at_midpoint(2.0, egui::vec2(50.0, -30.0));
}

// 触屏路径同样精确切割（zoom=2）。
#[test]
fn eraser_touch_cuts_line_at_midpoint_zoom2() {
    let (mut app, ctx) = eraser_line_app();
    eraser_line_view(&mut app, &ctx, 2.0, egui::vec2(50.0, -30.0));
    let before = app.session.document.clone();
    let size = Vec2::new(1000.0, 700.0);
    let start = eraser_screen(&app, (300.0, 250.0));
    let end = eraser_screen(&app, (308.0, 256.0));
    frame(
        &mut app,
        &ctx,
        size,
        vec![touch_event(1, egui::TouchPhase::Start, start.x, start.y)],
        false,
    );
    frame(
        &mut app,
        &ctx,
        size,
        vec![touch_event(1, egui::TouchPhase::Move, end.x, end.y)],
        false,
    );
    assert_eq!(
        line_pieces(erased_document(&app)).len(),
        2,
        "触控预览应切出两段"
    );
    frame(
        &mut app,
        &ctx,
        size,
        vec![touch_event(1, egui::TouchPhase::End, end.x, end.y)],
        false,
    );
    assert!(app.gesture.is_none());
    assert_eq!(app.session.document.revision, before.revision + 1);
    assert_eq!(line_pieces(&app.session.document).len(), 2);
}
