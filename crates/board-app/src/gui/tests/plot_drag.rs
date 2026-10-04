fn merge_highlights(output: &egui::FullOutput) -> Vec<(Rect, Color32)> {
    output
        .shapes
        .iter()
        .filter_map(|shape| {
            if let egui::Shape::Rect(rect) = &shape.shape
                && rect.stroke.width == 3.0
                && [Color32::LIGHT_GREEN, Color32::YELLOW].contains(&rect.stroke.color)
            {
                Some((rect.rect, rect.stroke.color))
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn plot_drag_resize_preview_commit_and_history_never_resample() {
    let expressions = [
        "x",
        "y^2=x",
        "x^2+y^2=9",
        "(x-1)^2/9+(y+2)^2/4=1",
        "x*y=1",
        "x^2+y^2=0",
        "x^2+y^2=-1",
    ];
    let (mut app, ctx) = plot_drag_app(vec![
        drag_plot("source", 100.0, &expressions),
        drag_plot("other", 600.0, &["x^2"]),
    ]);
    let size = Vec2::new(800.0, 600.0);
    frame(&mut app, &ctx, size, vec![], false);
    let baseline = app.renderer.plot_sample_calls();
    assert_eq!(baseline, 8);
    for resize in [false, true] {
        let before = app.session.document.current_page().objects.clone();
        if resize {
            // FunctionPlot has no pointer resize handles; exercise its existing Update path.
            let mut object = before[0].clone();
            if let ObjectKind::FunctionPlot { width, height, .. } = &mut object.kind {
                *width += 60.0;
                *height += 40.0;
            }
            app.apply(vec![Operation::Update { object }]);
        } else {
            let start = board_render::object_bounds(&before[0]).center();
            frame(
                &mut app,
                &ctx,
                size,
                vec![egui::Event::PointerMoved(start), pointer(start, true)],
                false,
            );
            assert!(app.gesture.is_some());
            let mut end = start;
            for n in 1..=12 {
                end = start + Vec2::new(3.0 * n as f32, 2.0 * n as f32);
                frame(
                    &mut app,
                    &ctx,
                    size,
                    vec![egui::Event::PointerMoved(end)],
                    false,
                );
                assert_eq!(app.session.document.current_page().objects, before);
                assert_ne!(app.preview().unwrap(), before[0]);
                assert_eq!(app.renderer.plot_sample_calls(), baseline);
            }
            release(&mut app, &ctx, end);
        }
        frame(&mut app, &ctx, size, vec![], false);
        let committed = app.session.document.current_page().objects.clone();
        assert_ne!(committed, before);
        assert_eq!(committed[1], before[1]);
        assert!(editing::hit_test(
            &committed[0],
            board_render::object_bounds(&committed[0]).center(),
            0.0
        ));
        assert_eq!(app.renderer.plot_sample_calls(), baseline);
        app.undo(false);
        frame(&mut app, &ctx, size, vec![], false);
        assert_eq!(app.session.document.current_page().objects, before);
        assert_eq!(app.renderer.plot_sample_calls(), baseline);
        app.undo(true);
        frame(&mut app, &ctx, size, vec![], false);
        assert_eq!(app.session.document.current_page().objects, committed);
        assert_eq!(app.renderer.plot_sample_calls(), baseline);
    }
    eprintln!(
        "GUI canvas sampling: baseline={baseline}, drag 12 previews+release+commit+undo+redo={}; size Update+undo+redo={} (delta=0)",
        app.renderer.plot_sample_calls(),
        app.renderer.plot_sample_calls()
    );
}

#[test]
fn plot_drag_frame_overlap_merges_outside_pointer_and_undoes_once() {
    let source = drag_plot("source", 100.0, &["x"]);
    let target = drag_plot("target", 300.0, &["x^2"]);
    let bounds = board_render::object_bounds(&target);
    let (mut app, ctx) = plot_drag_app(vec![source, target]);
    let before = app.session.document.current_page().objects.clone();
    let end = Pos2::new(220.0, 150.0);
    assert!(!bounds.contains(end));
    let output = drag_to(&mut app, &ctx, Pos2::new(110.0, 150.0), end);
    let expected = vec![(bounds.expand(4.0), Color32::LIGHT_GREEN)];
    assert_eq!(merge_highlights(&output), expected);
    assert_eq!(app.session.document.current_page().objects, before);
    let output = frame(
        &mut app,
        &ctx,
        Vec2::new(800.0, 600.0),
        vec![pointer(end, false)],
        false,
    );
    assert_eq!(merge_highlights(&output), expected);
    let merged = app.session.document.current_page().objects.clone();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].id, "target");
    let ObjectKind::FunctionPlot { expressions, .. } = &merged[0].kind else {
        panic!()
    };
    assert_eq!(expressions, &["x^2", "x"]);
    assert_eq!(board_render::object_bounds(&merged[0]), bounds);
    assert_eq!(app.session.document.revision, 1);
    app.undo(false);
    assert_eq!(app.session.document.current_page().objects, before);
    assert!(!app.session.history.can_undo());
    app.undo(true);
    assert_eq!(app.session.document.current_page().objects, merged);
    assert!(!app.session.history.can_redo());
}

#[test]
fn plot_drag_leaving_reducing_or_only_touching_does_not_merge() {
    for (source_x, end_x) in [(250.0, 170.0), (250.0, 230.0), (100.0, 210.0)] {
        let source = drag_plot("source", source_x, &["x"]);
        let target = drag_plot("target", 300.0, &["x^2"]);
        let (mut app, ctx) = plot_drag_app(vec![target.clone(), source]);
        let end = Pos2::new(end_x, 150.0);
        let output = drag_to(&mut app, &ctx, Pos2::new(source_x + 10.0, 150.0), end);
        assert!(merge_highlights(&output).is_empty());
        let moved = app.preview().unwrap();
        let output = frame(
            &mut app,
            &ctx,
            Vec2::new(800.0, 600.0),
            vec![pointer(end, false)],
            false,
        );
        assert!(merge_highlights(&output).is_empty());
        assert_eq!(
            app.session.document.current_page().objects,
            vec![target, moved]
        );
        assert_eq!(app.session.document.revision, 1);
    }
}

#[test]
fn plot_drag_existing_overlap_requires_strictly_larger_area() {
    for (dx, dy, merges) in [(0.0, 20.0, false), (20.0, 0.0, true)] {
        let source = drag_plot("source", 250.0, &["x"]);
        let mut target = drag_plot("target", 300.0, &["x^2"]);
        if let ObjectKind::FunctionPlot { height, .. } = &mut target.kind {
            *height = 200.0;
        }
        let (mut app, ctx) = plot_drag_app(vec![target, source]);
        let start = Pos2::new(260.0, 150.0);
        let end = start + Vec2::new(dx, dy);
        let output = drag_to(&mut app, &ctx, start, end);
        assert_eq!(!merge_highlights(&output).is_empty(), merges);
        frame(
            &mut app,
            &ctx,
            Vec2::new(800.0, 600.0),
            vec![pointer(end, false)],
            false,
        );
        assert_eq!(
            app.session.document.current_page().objects.len(),
            if merges { 1 } else { 2 }
        );
    }
}

#[test]
fn plot_drag_limit_warns_preserves_move_and_both_plots_atomically() {
    let mut target = drag_plot("target", 300.0, &["x"]);
    if let ObjectKind::FunctionPlot { expressions, .. } = &mut target.kind {
        *expressions = (0..16).map(|i| format!("x+{i}")).collect();
    }
    let bounds = board_render::object_bounds(&target);
    // 超限的顶层目标不能被跳过而合并到底层图。
    let low = drag_plot("low", 300.0, &["x^2"]);
    let (mut app, ctx) = plot_drag_app(vec![
        low.clone(),
        target.clone(),
        drag_plot("source", 100.0, &["x"]),
    ]);
    let before = app.session.document.current_page().objects.clone();
    let end = Pos2::new(220.0, 150.0);
    let output = drag_to(&mut app, &ctx, Pos2::new(110.0, 150.0), end);
    let expected = vec![(bounds.expand(4.0), Color32::YELLOW)];
    assert_eq!(merge_highlights(&output), expected);
    let moved = app.preview().unwrap();
    let output = frame(
        &mut app,
        &ctx,
        Vec2::new(800.0, 600.0),
        vec![pointer(end, false)],
        false,
    );
    assert_eq!(merge_highlights(&output), expected);
    assert_eq!(app.status, PLOT_MERGE_LIMIT_HINT);
    let after = vec![low, target, moved];
    assert_eq!(app.session.document.current_page().objects, after);
    assert_eq!(app.session.document.revision, 1);
    app.undo(false);
    assert_eq!(app.session.document.current_page().objects, before);
    assert!(!app.session.history.can_undo());
    app.undo(true);
    assert_eq!(app.session.document.current_page().objects, after);
}

#[test]
fn plot_drag_selects_topmost_target_excluding_source_and_other_kinds() {
    let low = drag_plot("low", 300.0, &["x^2"]);
    let high = drag_plot("high", 305.0, &["x^3"]);
    let bounds = board_render::object_bounds(&high);
    let source = drag_plot("source", 100.0, &["x"]);
    let text = BoardObject {
        id: "text".into(),
        kind: ObjectKind::Text {
            position: Point { x: 305.0, y: 100.0 },
            text: "label".into(),
            size: 24.0,
            color: Color::default(),
        },
    };
    let (mut app, ctx) = plot_drag_app(vec![low.clone(), high, source, text.clone()]);
    let end = Pos2::new(220.0, 150.0);
    let output = drag_to(&mut app, &ctx, Pos2::new(110.0, 150.0), end);
    assert_eq!(
        merge_highlights(&output),
        vec![(bounds.expand(4.0), Color32::LIGHT_GREEN)]
    );
    frame(
        &mut app,
        &ctx,
        Vec2::new(800.0, 600.0),
        vec![pointer(end, false)],
        false,
    );
    let objects = &app.session.document.current_page().objects;
    assert_eq!(objects.len(), 3);
    assert_eq!(objects[0], low);
    assert_eq!(objects[1].id, "high");
    assert_eq!(objects[2], text);
    assert_eq!(app.selected.as_deref(), Some("high"));
}

#[test]
fn non_plot_drag_never_shows_merge_hint_or_deletes_plot() {
    let source = BoardObject {
        id: "text".into(),
        kind: ObjectKind::Text {
            position: Point { x: 100.0, y: 100.0 },
            text: "text".into(),
            size: 24.0,
            color: Color::default(),
        },
    };
    let target = drag_plot("target", 300.0, &["x"]);
    let (mut app, ctx) = plot_drag_app(vec![target.clone(), source]);
    let end = Pos2::new(320.0, 110.0);
    let output = drag_to(&mut app, &ctx, Pos2::new(110.0, 110.0), end);
    assert!(merge_highlights(&output).is_empty());
    let moved = app.preview().unwrap();
    frame(
        &mut app,
        &ctx,
        Vec2::new(800.0, 600.0),
        vec![pointer(end, false)],
        false,
    );
    assert_eq!(
        app.session.document.current_page().objects,
        vec![target, moved]
    );
}

#[test]
fn plot_drag_release_frame_uses_current_pointer_for_hint_and_commit() {
    for (hover_x, release_x, merges) in [(220.0, 180.0, false), (180.0, 220.0, true)] {
        let (mut app, ctx) = plot_drag_app(vec![
            drag_plot("source", 100.0, &["x"]),
            drag_plot("target", 300.0, &["x^2"]),
        ]);
        let output = drag_to(
            &mut app,
            &ctx,
            Pos2::new(110.0, 150.0),
            Pos2::new(hover_x, 150.0),
        );
        assert_eq!(merge_highlights(&output).is_empty(), merges);
        let end = Pos2::new(release_x, 150.0);
        let output = frame(
            &mut app,
            &ctx,
            Vec2::new(800.0, 600.0),
            vec![egui::Event::PointerMoved(end), pointer(end, false)],
            false,
        );
        assert_eq!(!merge_highlights(&output).is_empty(), merges);
        assert_eq!(
            app.session.document.current_page().objects.len(),
            if merges { 1 } else { 2 }
        );
    }
}

#[test]
fn plot_drag_stale_document_page_or_revision_never_writes() {
    for stale in 0..3 {
        let (mut app, ctx) = plot_drag_app(vec![
            drag_plot("source", 100.0, &["x"]),
            drag_plot("target", 300.0, &["x^2"]),
        ]);
        let end = Pos2::new(220.0, 150.0);
        drag_to(&mut app, &ctx, Pos2::new(110.0, 150.0), end);
        match stale {
            0 => app.session.document.id = new_id(),
            1 => {
                app.session.document.add_page().unwrap();
            }
            _ => app.session.document.revision += 1,
        }
        let before = app.session.document.clone();
        let output = frame(
            &mut app,
            &ctx,
            Vec2::new(800.0, 600.0),
            vec![pointer(end, false)],
            false,
        );
        assert!(merge_highlights(&output).is_empty());
        assert_eq!(app.session.document, before);
        assert!(!app.session.history.can_undo());
        assert_eq!(app.status, "绘制期间文档已更新，本次操作已取消");
    }
}

#[test]
fn plot_drag_sixteen_distinct_expressions_accepts_duplicates() {
    let mut target = drag_plot("target", 300.0, &["x"]);
    if let ObjectKind::FunctionPlot { expressions, .. } = &mut target.kind {
        *expressions = (0..15).map(|i| format!("x+{i}")).collect();
        expressions.push("x+0".into());
    }
    let source = drag_plot("source", 100.0, &["x+0", "x+15", "x+15"]);
    let (mut app, ctx) = plot_drag_app(vec![source, target]);
    let end = Pos2::new(220.0, 150.0);
    let output = drag_to(&mut app, &ctx, Pos2::new(110.0, 150.0), end);
    assert_eq!(merge_highlights(&output)[0].1, Color32::LIGHT_GREEN);
    frame(
        &mut app,
        &ctx,
        Vec2::new(800.0, 600.0),
        vec![pointer(end, false)],
        false,
    );
    assert_eq!(app.session.document.current_page().objects.len(), 1);
    let ObjectKind::FunctionPlot { expressions, .. } =
        &app.session.document.current_page().objects[0].kind
    else {
        panic!()
    };
    assert_eq!(
        expressions,
        &(0..16).map(|i| format!("x+{i}")).collect::<Vec<_>>()
    );
}
