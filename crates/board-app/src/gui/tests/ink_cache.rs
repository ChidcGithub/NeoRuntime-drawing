#[test]
fn cache_multiple_real_sources_erase_result_click_regenerate_and_history() {
    let mut app = app();
    let ctx = egui::Context::default();
    let first = recognize_sample_at(&mut app, &ctx, Vec2::ZERO);
    let second = recognize_sample_at(&mut app, &ctx, Vec2::new(400.0, 220.0));
    assert_eq!(app.ink_cache.entries.len(), 2);
    assert_eq!(app.session.document.current_page().objects.len(), 8);
    plot_frame(&mut app, &ctx, vec![]);
    assert_eq!(app.controls.len(), 2);
    assert!(!app.controls[0].intersects(app.controls[1]));
    click_ink_icon(&mut app, &ctx);
    app.confirm_calculation(&ctx);
    drain_workers(&mut app);
    let result_id = app.ink_cache.get(&first).unwrap().result_id.clone();
    let original = app
        .session
        .document
        .current_page()
        .objects
        .iter()
        .find(|o| o.id == result_id)
        .unwrap()
        .clone();
    assert!(
        matches!(&original.kind, ObjectKind::Math { layout, .. } if features::layout_text(layout) == "= 2")
    );
    assert!(app.ink_cache.get(&second).is_some());
    app.tool = Tool::Select;
    app.controls.clear();
    let center = board_render::object_bounds(&original).center();
    eraser_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(center), pointer(center, true)],
    );
    assert_eq!(app.selected.as_deref(), Some(result_id.as_str()));
    let end = center + Vec2::new(100.0, 80.0);
    eraser_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
    eraser_frame(&mut app, &ctx, vec![pointer(end, false)]);
    assert!(app.ink_cache.get(&first).unwrap().result_present);
    assert!(app.ink_cache.get(&second).is_some());
    assert!(app.activate_candidate(&first));
    app.confirm_calculation(&ctx);
    assert!(!app.math_worker.busy());
    assert_eq!(app.session.document.current_page().objects.len(), 9);
    let moved = app
        .session
        .document
        .current_page()
        .objects
        .iter()
        .find(|object| object.id == result_id)
        .unwrap();
    assert_ne!(moved, &original);
    assert!(editing::hit_test(
        moved,
        board_render::object_bounds(moved).center(),
        0.0
    ));
    app.undo(false);
    assert!(app.ink_cache.get(&first).unwrap().result_present);
    app.undo(false);
    assert!(!app.ink_cache.get(&first).unwrap().result_present);
    app.undo(true);
    assert!(app.ink_cache.get(&first).unwrap().result_present);
    assert!(app.activate_candidate(&first));
    app.confirm_calculation(&ctx);
    assert!(!app.math_worker.busy());
    assert_eq!(app.session.document.current_page().objects.len(), 9);
    app.tool = Tool::Eraser;
    app.eraser = 2.0;
    let point = board_render::object_bounds(&original).center();
    eraser_frame(&mut app, &ctx, vec![]);
    eraser_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(point), pointer(point, true)],
    );
    app.sync_ink_cache();
    assert!(app.ink_cache.get(&first).unwrap().result_present);
    eraser_frame(&mut app, &ctx, vec![pointer(point, false)]);
    assert_eq!(app.ink_cache.entries.len(), 2);
    assert!(!app.ink_cache.get(&first).unwrap().result_present);
    click_ink_icon(&mut app, &ctx);
    drain_workers(&mut app);
    assert_eq!(
        app.session
            .document
            .current_page()
            .objects
            .iter()
            .find(|o| o.id == result_id),
        Some(&original)
    );
    app.undo(false);
    assert_eq!(app.session.document.current_page().objects.len(), 8);
    app.undo(true);
    assert_eq!(app.session.document.current_page().objects.len(), 9);
    assert!(app.activate_candidate(&second));
    app.confirm_calculation(&ctx);
    drain_workers(&mut app);
    assert_eq!(app.session.document.current_page().objects.len(), 10);
}

#[test]
fn cache_local_changes_page_backend_and_same_identity_open() {
    let mut app = app();
    let ctx = egui::Context::default();
    let first = recognize_sample_at(&mut app, &ctx, Vec2::ZERO);
    let second = recognize_sample_at(&mut app, &ctx, Vec2::new(400.0, 220.0));
    app.add(ObjectKind::Text {
        position: Point {
            x: 1400.0,
            y: 1200.0,
        },
        text: "far".into(),
        size: 20.0,
        color: Color::default(),
    });
    assert_eq!(app.ink_cache.entries.len(), 2);
    app.tool = Tool::Select;
    app.cancel_ink();
    app.session
        .history
        .edit(&mut app.session.document, |document| document.add_page())
        .unwrap();
    app.changed();
    assert_eq!(app.ink_cache.entries.len(), 2);
    app.session.document.set_current_page(0).unwrap();
    app.changed();
    app.set_hwr_backend(HwrBackend::TexTeller);
    assert!(app.activate_candidate(&first));
    assert!(app.ink_cache.get(&first).unwrap().backend == HwrBackend::Template);
    app.expression = "1/2+1/3".into();
    app.save_candidate_edit();
    assert!(app.activate_candidate(&second));
    assert_eq!(app.expression, "1+1");
    assert!(app.activate_candidate(&first));
    assert_eq!(app.expression, "1/2+1/3");
    app.add(ObjectKind::Text {
        position: Point { x: 100.0, y: 90.0 },
        text: "near".into(),
        size: 20.0,
        color: Color::default(),
    });
    assert!(app.ink_cache.get(&first).is_none());
    assert!(app.ink_cache.get(&second).is_some());
    let path = std::env::temp_dir().join(format!("cache-{}.neoboard", new_id()));
    app.path = path.display().to_string();
    assert!(app.save());
    assert!(app.ink_cache.get(&second).is_some());
    let epoch = app.ink_cache.epoch;
    let before = ContextToken::capture(&app.session.document);
    app.incoming_message(
        Request::new(
            "neo:cache-open",
            "document.open",
            serde_json::json!({"path": path, "discard_unsaved": true}),
        )
        .unwrap()
        .into(),
    );
    std::fs::remove_file(path).unwrap();
    assert_eq!(before, ContextToken::capture(&app.session.document));
    assert!(app.ink_cache.entries.is_empty());
    assert_ne!(epoch, app.ink_cache.epoch);
}

#[test]
fn cache_neighborhood_move_modify_delete_only_affects_local_sources() {
    for change in 0..5 {
        let mut app = app();
        let ctx = egui::Context::default();
        let near = BoardObject {
            id: new_id(),
            kind: ObjectKind::Text {
                position: Point { x: 40.0, y: 65.0 },
                text: "near".into(),
                size: 12.0,
                color: Color::default(),
            },
        };
        app.apply(vec![Operation::Add {
            object: near.clone(),
        }]);
        let first = recognize_sample_at(&mut app, &ctx, Vec2::ZERO);
        let second = recognize_sample_at(&mut app, &ctx, Vec2::new(400.0, 220.0));
        let mut object = if change < 3 {
            near
        } else {
            app.ink_cache.get(&first).unwrap().source[0].clone()
        };
        let operation = match change {
            0 => {
                if let ObjectKind::Text { position, .. } = &mut object.kind {
                    position.x = 1500.0;
                }
                Operation::Update { object }
            }
            1 => {
                if let ObjectKind::Text { text, .. } = &mut object.kind {
                    *text = "edited".into();
                }
                Operation::Update { object }
            }
            2 | 3 => Operation::Delete { id: object.id },
            _ => {
                if let ObjectKind::Stroke { style, .. } = &mut object.kind {
                    style.width += 1.0;
                }
                Operation::Update { object }
            }
        };
        app.apply(vec![operation]);
        assert!(app.ink_cache.get(&first).is_none(), "change {change}");
        assert!(app.ink_cache.get(&second).is_some(), "change {change}");
    }
    let mut app = app();
    let ctx = egui::Context::default();
    let first = recognize_sample_at(&mut app, &ctx, Vec2::ZERO);
    let mut distant = BoardObject {
        id: new_id(),
        kind: ObjectKind::Text {
            position: Point { x: 1500.0, y: 90.0 },
            text: "moving".into(),
            size: 12.0,
            color: Color::default(),
        },
    };
    app.apply(vec![Operation::Add {
        object: distant.clone(),
    }]);
    assert!(app.ink_cache.get(&first).is_some());
    if let ObjectKind::Text { position, .. } = &mut distant.kind {
        position.x = 100.0;
    }
    app.apply(vec![Operation::Update { object: distant }]);
    assert!(app.ink_cache.get(&first).is_none());
}

#[test]
fn cache_keeps_more_than_sixty_four_real_candidates_without_lru_eviction() {
    let mut app = app();
    let ctx = egui::Context::default();
    let first = recognize_sample_at(&mut app, &ctx, Vec2::ZERO);
    for index in 1..70 {
        recognize_sample_at(&mut app, &ctx, Vec2::new(index as f32 * 400.0, 0.0));
    }
    assert_eq!(app.ink_cache.entries.len(), 70);
    assert!(app.activate_candidate(&first));
    app.confirm_calculation(&ctx);
    drain_workers(&mut app);
    assert_eq!(app.ink_cache.entries.len(), 70);
    assert!(app.ink_cache.get(&first).unwrap().result_present);
    let epoch = app.ink_cache.epoch;
    app.incoming_message(
        Request::new(
            "neo:cache-new",
            "document.new",
            serde_json::json!({"discard_unsaved": true}),
        )
        .unwrap()
        .into(),
    );
    assert!(app.ink_cache.entries.is_empty());
    assert_ne!(epoch, app.ink_cache.epoch);
}

#[test]
fn cache_strict_inflight_revision_rejects_but_far_change_can_retry() {
    for nearby in [false, true] {
        let mut app = app();
        let ctx = egui::Context::default();
        let id = recognize_sample_at(&mut app, &ctx, Vec2::ZERO);
        let (tx, rx) = mpsc::channel();
        app.start_math(&ctx, move || {
            rx.recv().unwrap();
            Ok(("must not appear".into(), Some(text_kind("stale"))))
        });
        app.running_candidate = Some((id.clone(), app.ink_cache.epoch));
        app.add(ObjectKind::Text {
            position: Point {
                x: if nearby { 100.0 } else { 1400.0 },
                y: 90.0,
            },
            text: "change".into(),
            size: 12.0,
            color: Color::default(),
        });
        tx.send(()).unwrap();
        drain_workers(&mut app);
        assert_eq!(app.session.document.current_page().objects.len(), 5);
        assert_eq!(app.ink_cache.get(&id).is_some(), !nearby);
        if !nearby {
            assert!(app.activate_candidate(&id));
            app.confirm_calculation(&ctx);
            drain_workers(&mut app);
            assert_eq!(app.session.document.current_page().objects.len(), 6);
        }
    }
}

#[test]
fn cache_source_erase_preview_cancel_does_not_invalidate_until_commit() {
    let mut app = app();
    let ctx = egui::Context::default();
    let id = recognize_sample_at(&mut app, &ctx, Vec2::ZERO);
    app.tool = Tool::Eraser;
    let point = Pos2::new(80.0, 100.0);
    eraser_frame(&mut app, &ctx, vec![]);
    eraser_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(point), pointer(point, true)],
    );
    app.sync_ink_cache();
    assert!(app.ink_cache.get(&id).is_some());
    app.gesture = None;
    eraser_frame(&mut app, &ctx, vec![pointer(point, false)]);
    assert!(app.ink_cache.get(&id).is_some());
    eraser_frame(&mut app, &ctx, vec![pointer(point, true)]);
    eraser_frame(&mut app, &ctx, vec![pointer(point, false)]);
    assert!(app.ink_cache.get(&id).is_none());
}

#[test]
fn cache_corrected_exact_equation_and_function_regenerate_single_object() {
    for expression in ["1/2+1/3", "2x+3=x-1", "y=x^2", "y²=x", "y-x=1", "x²+y²=4"] {
        let mut app = app();
        let ctx = egui::Context::default();
        let id = recognize_sample_at(&mut app, &ctx, Vec2::ZERO);
        app.expression = expression.into();
        app.confirm_calculation(&ctx);
        drain_workers(&mut app);
        let result = app
            .session
            .document
            .current_page()
            .objects
            .last()
            .unwrap()
            .clone();
        match expression {
            "1/2+1/3" => assert!(
                matches!(&result.kind, ObjectKind::Math { layout, .. } if features::layout_text(layout).contains("5/6"))
            ),
            "2x+3=x-1" => assert!(
                matches!(&result.kind, ObjectKind::Text { text, .. } if text.contains("-4"))
            ),
            _ => assert!(
                matches!(&result.kind, ObjectKind::FunctionPlot { expressions, .. } if expressions == &[board_math::plot_expression(expression).unwrap()])
            ),
        }
        app.apply(vec![Operation::Delete {
            id: result.id.clone(),
        }]);
        assert!(app.ink_cache.get(&id).is_some());
        click_ink_icon(&mut app, &ctx);
        drain_workers(&mut app);
        assert_eq!(app.session.document.current_page().objects.len(), 5);
        assert_eq!(
            app.session.document.current_page().objects.last(),
            Some(&result)
        );
        app.undo(false);
        assert_eq!(app.session.document.current_page().objects.len(), 4);
        app.undo(true);
        assert_eq!(app.session.document.current_page().objects.len(), 5);
    }
}
