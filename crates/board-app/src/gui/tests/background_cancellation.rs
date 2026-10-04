#[test]
fn cancelled_background_result_never_writes() {
    let mut app = app();
    let ctx = egui::Context::default();
    let (tx, rx) = mpsc::channel();
    app.start_math(&ctx, move || {
        rx.recv().unwrap();
        Ok((
            "结果".into(),
            Some(ObjectKind::Text {
                position: Point::default(),
                text: "不能写入".into(),
                size: 24.0,
                color: Color::default(),
            }),
        ))
    });
    assert!(app.math_worker.busy());
    app.cancel_ink();
    tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while app.math_worker.busy() && Instant::now() < deadline {
        app.poll_workers(&egui::Context::default());
        std::thread::yield_now();
    }
    assert!(!app.math_worker.busy());
    assert!(app.session.document.current_page().objects.is_empty());
}
