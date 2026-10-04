#[test]
fn implicit_mixed_intersection_ui_reports_unsupported_without_markers() {
    let (mut app, ctx) = plot_drag_app(vec![drag_plot("plot", 100.0, &["x", "y^2=x"])]);
    app.selected = Some("plot".into());
    plot_frame(&mut app, &ctx, vec![]);
    drain_workers(&mut app);
    let shapes = plot_frame(&mut app, &ctx, vec![]);
    assert!(shapes.contains("暂不支持隐式曲线"));
    assert!(shapes.contains("未执行搜索"));
    assert!(
        app.plot_points
            .as_ref()
            .unwrap()
            .report
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .candidates
            .is_empty()
    );
    assert_eq!(app.session.document.current_page().objects.len(), 1);
}

#[test]
fn intersection_headless_selection_wait_click_and_move() {
    let (mut app, ctx) = plot_drag_app(vec![drag_plot("plot", 100.0, &["x", "x^2"])]);
    app.status = "保存失败：保留此错误".into();
    plot_frame(&mut app, &ctx, vec![]);
    let click = Pos2::new(125.0, 150.0);
    plot_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(click), pointer(click, true)],
    );
    plot_frame(&mut app, &ctx, vec![pointer(click, false)]);
    assert_eq!(app.selected.as_deref(), Some("plot"));
    // 不轮询完成结果，让真实 worker 完成与否均能确定性检查等待界面。
    plot_frame(&mut app, &ctx, vec![]);
    let waiting = plot_frame(&mut app, &ctx, vec![]);
    assert!(waiting.contains("交点计算中"));
    drain_workers(&mut app);
    let found = plot_frame(&mut app, &ctx, vec![]);
    assert!(found.contains("找到 2 候选"));
    assert!(found.contains("非完备"));
    let marker = app
        .plot_points
        .as_ref()
        .unwrap()
        .report
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap()
        .candidates[0]
        .0;
    plot_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(marker), pointer(marker, true)],
    );
    plot_frame(&mut app, &ctx, vec![pointer(marker, false)]);
    let text = plot_frame(&mut app, &ctx, vec![]);
    assert!(
        text.contains("交点约 (0.000000, 0.000000)"),
        "coordinate={:?}; selected={:?}; controls={:?}",
        app.plot_points.as_ref().and_then(|s| s.coordinate.as_ref()),
        app.selected,
        app.controls
    );
    assert_eq!(app.status, "保存失败：保留此错误");
    let mut moved = app.selected_object().unwrap();
    editing::translate(&mut moved, Point { x: 40.0, y: 30.0 });
    app.apply(vec![Operation::Update { object: moved }]);
    assert!(app.plot_points.is_none());
    plot_frame(&mut app, &ctx, vec![]);
    drain_workers(&mut app);
    let state = app.plot_points.as_ref().unwrap();
    assert!(state.coordinate.is_none());
    let new_marker = state.report.as_ref().unwrap().as_ref().unwrap().candidates[0].0;
    assert_eq!(new_marker, marker + Vec2::new(40.0, 30.0));
}

#[test]
fn intersection_stale_worker_and_cached_reports_never_display() {
    for cached in [false, true] {
        for change in 0..5 {
            let object = drag_plot("plot", 100.0, &["x"]);
            let (mut app, ctx) =
                plot_drag_app(vec![object.clone(), drag_plot("other", 300.0, &["x^2"])]);
            app.selected = Some(object.id.clone());
            let context = ContextToken::capture(&app.session.document);
            app.plot_points = Some(PlotState {
                context: context.clone(),
                object: object.clone(),
                report: cached.then(|| Ok(features::intersections(&object))),
                coordinate: Some("旧坐标不可展示".into()),
            });
            let (tx, rx) = mpsc::channel();
            if !cached {
                app.plot_worker.start(ctx.clone(), move || {
                    rx.recv().unwrap();
                    let report = features::intersections(&object);
                    (context, object, report)
                });
            }
            match change {
                0 => app.session.document.id = new_id(),
                1 => app.session.document.pages[0].id = new_id(),
                2 => app.session.document.revision += 1,
                3 => app.selected = Some("other".into()),
                _ => editing::translate(
                    &mut app.session.document.pages[0].objects[0],
                    Point { x: 10.0, y: 0.0 },
                ),
            }
            app.sync_plot_state();
            assert!(app.plot_points.is_none());
            if !cached {
                tx.send(()).unwrap();
            }
            drain_workers(&mut app);
            assert!(app.plot_points.is_none());
            let shapes = plot_frame(&mut app, &ctx, vec![]);
            assert!(!shapes.contains("旧坐标不可展示"));
            drain_workers(&mut app);
        }
    }
}

#[test]
fn intersection_headless_partial_failure_preserves_save_error_and_markers() {
    let (mut app, ctx) = plot_drag_app(vec![drag_plot("plot", 100.0, &["x", "bogus(x)"])]);
    app.selected = Some("plot".into());
    app.status = "保存失败".into();
    plot_frame(&mut app, &ctx, vec![]);
    drain_workers(&mut app);
    plot_frame(&mut app, &ctx, vec![]);
    let text = plot_frame(&mut app, &ctx, vec![]);
    assert!(text.contains("部分搜索失败"));
    assert!(text.contains("找到 1 候选"));
    assert_eq!(app.status, "保存失败");
}
