// 触控多指书写：全部手指走 Touch 事件路径（每指独立手势，上限 5）；
// 无指按下时的指针按下回到鼠标/触控笔模式（touch_writing 粘性标志）。

fn touch_event(id: u64, phase: egui::TouchPhase, x: f32, y: f32) -> egui::Event {
    egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(id),
        phase,
        pos: egui::pos2(x, y),
        force: None,
    }
}

fn stroke_count(app: &BoardApp) -> usize {
    app.session.document.current_page().objects.len()
}

#[test]
fn single_touch_writes_a_stroke() {
    let mut app = app();
    let ctx = egui::Context::default();
    board_frame(
        &mut app,
        &ctx,
        vec![touch_event(1, egui::TouchPhase::Start, 100.0, 100.0)],
    );
    board_frame(
        &mut app,
        &ctx,
        vec![touch_event(1, egui::TouchPhase::Move, 160.0, 100.0)],
    );
    board_frame(
        &mut app,
        &ctx,
        vec![touch_event(1, egui::TouchPhase::End, 200.0, 100.0)],
    );
    assert_eq!(stroke_count(&app), 1);
    assert!(matches!(
        app.session.document.current_page().objects[0].kind,
        ObjectKind::Stroke { .. }
    ));
    // 触控模式粘性：指针抬起后 touch_writing 保持，直到鼠标按下。
    assert!(app.touch_writing);
}

#[test]
fn two_fingers_write_two_independent_strokes() {
    let mut app = app();
    let ctx = egui::Context::default();
    // 首指落笔。
    board_frame(
        &mut app,
        &ctx,
        vec![touch_event(1, egui::TouchPhase::Start, 100.0, 100.0)],
    );
    // 第二根手指落地（附加指触控路径起手势）。
    board_frame(
        &mut app,
        &ctx,
        vec![touch_event(2, egui::TouchPhase::Start, 100.0, 300.0)],
    );
    // 两指同时移动。
    board_frame(
        &mut app,
        &ctx,
        vec![
            touch_event(1, egui::TouchPhase::Move, 160.0, 100.0),
            touch_event(2, egui::TouchPhase::Move, 160.0, 300.0),
        ],
    );
    // 首指抬手提交一笔，附加指继续。
    board_frame(
        &mut app,
        &ctx,
        vec![touch_event(1, egui::TouchPhase::End, 200.0, 100.0)],
    );
    board_frame(
        &mut app,
        &ctx,
        vec![touch_event(2, egui::TouchPhase::Move, 220.0, 300.0)],
    );
    // 附加指抬手提交第二笔。
    board_frame(
        &mut app,
        &ctx,
        vec![touch_event(2, egui::TouchPhase::End, 260.0, 300.0)],
    );
    assert_eq!(stroke_count(&app), 2, "each finger commits its own stroke");
    let objects = &app.session.document.current_page().objects;
    assert!(matches!(objects[0].kind, ObjectKind::Stroke { .. }));
    assert!(matches!(objects[1].kind, ObjectKind::Stroke { .. }));
}

#[test]
fn touch_cancel_drops_the_stroke() {
    let mut app = app();
    let ctx = egui::Context::default();
    board_frame(
        &mut app,
        &ctx,
        vec![touch_event(1, egui::TouchPhase::Start, 100.0, 100.0)],
    );
    board_frame(
        &mut app,
        &ctx,
        vec![touch_event(2, egui::TouchPhase::Start, 100.0, 300.0)],
    );
    board_frame(
        &mut app,
        &ctx,
        vec![touch_event(2, egui::TouchPhase::Move, 160.0, 300.0)],
    );
    // 系统取消该触点（手势被系统接管）：该指笔迹丢弃。
    board_frame(
        &mut app,
        &ctx,
        vec![touch_event(2, egui::TouchPhase::Cancel, 160.0, 300.0)],
    );
    board_frame(
        &mut app,
        &ctx,
        vec![touch_event(2, egui::TouchPhase::End, 260.0, 300.0)],
    );
    board_frame(
        &mut app,
        &ctx,
        vec![touch_event(1, egui::TouchPhase::End, 200.0, 100.0)],
    );
    assert_eq!(stroke_count(&app), 1, "cancelled finger writes nothing");
}

#[test]
fn finger_limit_caps_concurrent_fingers() {
    let mut app = app();
    let ctx = egui::Context::default();
    for finger in 1..=6 {
        board_frame(
            &mut app,
            &ctx,
            vec![touch_event(
                finger,
                egui::TouchPhase::Start,
                100.0,
                100.0 + finger as f32 * 20.0,
            )],
        );
    }
    assert_eq!(app.multi_gestures.len(), 5, "five fingers maximum");
    for finger in 1..=6 {
        board_frame(
            &mut app,
            &ctx,
            vec![touch_event(
                finger,
                egui::TouchPhase::End,
                120.0,
                100.0 + finger as f32 * 20.0,
            )],
        );
    }
}

#[test]
fn mouse_press_after_touch_returns_to_pointer_mode() {
    let mut app = app();
    let ctx = egui::Context::default();
    // 触控写一笔。
    board_frame(
        &mut app,
        &ctx,
        vec![touch_event(1, egui::TouchPhase::Start, 100.0, 100.0)],
    );
    board_frame(
        &mut app,
        &ctx,
        vec![touch_event(1, egui::TouchPhase::End, 140.0, 100.0)],
    );
    assert!(app.touch_writing);
    // 无指按下时的指针按下：回到指针模式并起笔。
    board_frame(
        &mut app,
        &ctx,
        vec![pointer(egui::pos2(300.0, 300.0), true)],
    );
    assert!(!app.touch_writing);
    board_frame(
        &mut app,
        &ctx,
        vec![pointer(egui::pos2(360.0, 300.0), true)],
    );
    board_frame(
        &mut app,
        &ctx,
        vec![pointer(egui::pos2(360.0, 300.0), false)],
    );
    assert_eq!(stroke_count(&app), 2, "mouse stroke after touch stroke");
    assert!(matches!(
        app.session.document.current_page().objects[1].kind,
        ObjectKind::Stroke { .. }
    ));
}
