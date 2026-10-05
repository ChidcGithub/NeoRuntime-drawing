fn handwriting_commit_local_pen(app: &mut BoardApp, ctx: &egui::Context) {
    app.tool = Tool::Pen;
    drag_to(app, ctx, Pos2::new(100.0, 100.0), Pos2::new(130.0, 172.0));
    release(app, ctx, Pos2::new(130.0, 172.0));
    assert!(app.gesture.is_none());
}

fn handwriting_synthesized_five(app: &BoardApp) -> ObjectKind {
    app.handwriting
        .profile
        .render_adaptive(&text_kind("5"), "5", |ch| {
            crate::handwriting_fallback::sample(ch, None)
        })
        .unwrap()
}

fn handwriting_geometry(kind: &ObjectKind) -> Vec<Vec<(f32, f32)>> {
    let ObjectKind::Handwritten { strokes, .. } = kind else {
        panic!("expected synthesized handwriting");
    };
    strokes
        .iter()
        .map(|stroke| stroke.points.iter().map(|p| (p.x, p.y)).collect())
        .collect()
}

#[test]
fn handwriting_raw_board_app_defaults_on_and_synthesizes_without_samples() {
    assert!(handwriting_ui::HandwritingUi::default().enabled);
    let mut app = default_app();
    assert!(app.handwriting.enabled);
    assert!(app.ink_math_mode == InkMathMode::Off);
    assert_eq!(app.handwriting.profile.counts(), (0, 0));
    assert_eq!(app.handwriting.profile.learned_strokes(), 0);
    app.export_resources = Default::default();
    let profile = app.handwriting.profile.to_json().unwrap();
    app.start_math(&egui::Context::default(), || {
        Ok(("5".into(), Some(text_kind("5"))))
    });
    drain_workers(&mut app);
    let objects = &app.session.document.current_page().objects;
    assert_eq!(objects.len(), 1);
    assert_handwriting_answer(&objects[0].kind, "5", &text_kind("5"));
    assert_eq!(app.math_result, "5");
    assert_eq!(app.handwriting.profile.to_json().unwrap(), profile);
}

#[test]
fn handwriting_local_pen_learns_with_hwr_off_only_after_commit_and_changes_geometry() {
    let mut app = default_app();
    let ctx = egui::Context::default();
    assert!(app.ink_math_mode == InkMathMode::Off);
    let before = handwriting_synthesized_five(&app);
    let document = app.session.document.clone();
    let profile = app.handwriting.profile.to_json().unwrap();
    app.tool = Tool::Pen;
    drag_to(
        &mut app,
        &ctx,
        Pos2::new(100.0, 100.0),
        Pos2::new(130.0, 172.0),
    );
    assert_eq!(app.session.document, document);
    assert!(!app.session.history.can_undo());
    assert_eq!(app.handwriting.profile.to_json().unwrap(), profile);
    release(&mut app, &ctx, Pos2::new(130.0, 172.0));
    assert_eq!(app.handwriting.profile.learned_strokes(), 1);
    assert_eq!(app.handwriting.profile.counts(), (0, 0));
    assert!(!app.hwr_worker.busy());
    assert!(!app.math_worker.busy());
    assert!(app.ink.is_empty());
    let objects = app.session.document.current_page().objects.clone();
    assert_eq!(objects.len(), 1);
    assert!(matches!(objects[0].kind, ObjectKind::Stroke { .. }));
    assert_eq!(app.session.document.revision, document.revision + 1);
    let learned = app.handwriting.profile.to_json().unwrap();
    let after = handwriting_synthesized_five(&app);
    assert_ne!(handwriting_geometry(&before), handwriting_geometry(&after));
    assert_eq!(app.handwriting.profile.to_json().unwrap(), learned);

    app.undo(false);
    assert!(app.session.document.current_page().objects.is_empty());
    assert!(
        !app.session.history.can_undo(),
        "learning must not add history"
    );
    assert!(app.session.history.can_redo());
    assert_eq!(app.handwriting.profile.to_json().unwrap(), learned);
    app.undo(true);
    assert_eq!(app.session.document.current_page().objects, objects);
    assert!(!app.session.history.can_redo());
    assert_eq!(app.handwriting.profile.to_json().unwrap(), learned);

    app.start_math(&ctx, || Ok(("5".into(), Some(text_kind("5")))));
    drain_workers(&mut app);
    assert_eq!(app.session.document.current_page().objects.len(), 2);
    assert_eq!(app.session.document.current_page().objects[1].kind, after);
    assert_eq!(app.handwriting.profile.to_json().unwrap(), learned);
}

#[test]
fn handwriting_disabled_pauses_learning_and_reenabled_pen_resumes() {
    let mut app = default_app();
    let ctx = egui::Context::default();
    handwriting_commit_local_pen(&mut app, &ctx);
    assert_eq!(app.handwriting.profile.learned_strokes(), 1);
    let learned = app.handwriting.profile.to_json().unwrap();
    app.handwriting.enabled = false;
    handwriting_commit_local_pen(&mut app, &ctx);
    assert_eq!(app.session.document.current_page().objects.len(), 2);
    assert_eq!(app.session.document.revision, 2);
    assert_eq!(app.handwriting.profile.to_json().unwrap(), learned);
    app.handwriting.enabled = true;
    handwriting_commit_local_pen(&mut app, &ctx);
    assert_eq!(app.session.document.current_page().objects.len(), 3);
    assert_eq!(app.session.document.revision, 3);
    assert_eq!(app.handwriting.profile.learned_strokes(), 2);
}

#[test]
fn handwriting_add_import_and_move_existing_ink_do_not_teach() {
    let mut app = default_app();
    let ctx = egui::Context::default();
    handwriting_commit_local_pen(&mut app, &ctx);
    let learned = app.handwriting.profile.to_json().unwrap();
    let original = app.session.document.current_page().objects[0].clone();
    app.tool = Tool::Select;
    drag_to(
        &mut app,
        &ctx,
        Pos2::new(115.0, 136.0),
        Pos2::new(215.0, 216.0),
    );
    release(&mut app, &ctx, Pos2::new(215.0, 216.0));
    let mut moved = original.clone();
    editing::translate(&mut moved, Point { x: 100.0, y: 80.0 });
    assert_eq!(app.session.document.current_page().objects, vec![moved]);
    assert_eq!(app.handwriting.profile.to_json().unwrap(), learned);
    app.add(original.kind.clone());
    assert_eq!(app.session.document.current_page().objects.len(), 2);
    assert_eq!(app.handwriting.profile.to_json().unwrap(), learned);
    let page = app.session.document.current_page().id.clone();
    app.incoming_message(
        Request::new(
            "neo:handwriting-add",
            "objects.apply",
            serde_json::json!({
                "document_id": app.session.document.id,
                "page_id": page,
                "expected_revision": app.session.document.revision,
                "operations": [{"op": "add", "object": {
                    "id": "remote-ink", "kind": original.kind
                }}]
            }),
        )
        .unwrap()
        .into(),
    );
    assert_eq!(app.session.document.current_page().objects.len(), 3);
    assert_eq!(app.handwriting.profile.to_json().unwrap(), learned);

    let path = std::env::temp_dir().join(format!("handwriting-import-{}.neoboard", new_id()));
    app.session.save_document(&path).unwrap();
    let document = app.session.document.clone();
    let mut imported = default_app();
    let empty_profile = imported.handwriting.profile.to_json().unwrap();
    imported.incoming_message(
        Request::new(
            "neo:handwriting-open",
            "document.open",
            serde_json::json!({
                "path": path, "discard_unsaved": true
            }),
        )
        .unwrap()
        .into(),
    );
    std::fs::remove_file(path).unwrap();
    assert_eq!(imported.session.document, document);
    frame(
        &mut imported,
        &egui::Context::default(),
        Vec2::new(800.0, 600.0),
        vec![],
        false,
    );
    assert_eq!(
        imported.handwriting.profile.to_json().unwrap(),
        empty_profile
    );
    assert!(!imported.session.history.can_undo());
}

#[test]
fn handwriting_failed_stale_and_cancelled_pen_gestures_do_not_teach() {
    for change in 0..5 {
        let mut app = default_app();
        let ctx = egui::Context::default();
        if change == 0 {
            app.session.document.revision = u64::MAX;
            app.session.history = board_core::History::new(&app.session.document);
        }
        app.tool = Tool::Pen;
        drag_to(
            &mut app,
            &ctx,
            Pos2::new(100.0, 100.0),
            Pos2::new(130.0, 172.0),
        );
        match change {
            0 => {}
            1 => app.gesture.as_mut().unwrap().revision += 1,
            2 => app.gesture.as_mut().unwrap().document = new_id(),
            3 => app.gesture.as_mut().unwrap().page = new_id(),
            _ => app.gesture = None,
        }
        let document = app.session.document.clone();
        let profile = app.handwriting.profile.to_json().unwrap();
        app.finish_gesture();
        assert!(app.gesture.is_none());
        assert_eq!(app.session.document, document, "change {change}");
        assert_eq!(
            app.handwriting.profile.to_json().unwrap(),
            profile,
            "change {change}"
        );
        assert!(!app.session.history.can_undo(), "change {change}");
        assert!(!app.session.history.can_redo(), "change {change}");
        if change == 0 {
            assert_eq!(app.status, board_core::Error::RevisionOverflow.to_string());
        }
    }
}

#[test]
fn handwriting_drawing_defaults_off_and_forced_enable_does_not_learn_pen() {
    let mut session = Session::new(AppMode::Drawing.kind());
    session.handle(
        Request::new(
            "neo:setup",
            "configure",
            serde_json::json!({
                "classroom_safe": true,
                "desktop_capture_allowed": false,
                "agent_allowed": false
            }),
        )
        .unwrap(),
    );
    let mut app = BoardApp::new(AppMode::Drawing, session, false, None, Default::default());
    assert!(!app.handwriting.enabled);
    app.handwriting.enabled = true;
    let profile = app.handwriting.profile.to_json().unwrap();
    handwriting_commit_local_pen(&mut app, &egui::Context::default());
    let objects = &app.session.document.current_page().objects;
    assert_eq!(objects.len(), 1);
    assert!(matches!(objects[0].kind, ObjectKind::Stroke { .. }));
    assert_eq!(app.session.document.revision, 1);
    assert_eq!(app.handwriting.profile.learned_strokes(), 0);
    assert_eq!(app.handwriting.profile.to_json().unwrap(), profile);
    app.undo(false);
    assert!(app.session.document.current_page().objects.is_empty());
    assert!(!app.session.history.can_undo());
}

#[test]
fn handwriting_point_budget_falls_back_atomically_with_same_candidate_identity() {
    for candidate in [false, true] {
        let mut app = default_app();
        let ctx = egui::Context::default();
        let candidate_id = candidate.then(|| recognize_sample_at(&mut app, &ctx, Vec2::ZERO));
        let result_id = candidate_id
            .as_ref()
            .map(|id| app.ink_cache.get(id).unwrap().result_id.clone());
        let source_points: usize = app
            .session
            .document
            .current_page()
            .objects
            .iter()
            .map(|object| {
                let ObjectKind::Stroke { points, .. } = &object.kind else {
                    panic!("expected candidate source ink");
                };
                points.len()
            })
            .sum();
        // A remote shape reserves capacity without becoming candidate-neighborhood ink.
        app.session.document.pages[0].objects.push(BoardObject {
            id: "point-capacity".into(),
            kind: ObjectKind::Shape {
                shape: ShapeKind::Line,
                points: vec![
                    Point {
                        x: 10_000.0,
                        y: 10_000.0
                    };
                    board_core::MAX_DOCUMENT_POINTS - 1 - source_points
                ],
                style: Style::default(),
            },
        });
        app.session.document.validate().unwrap();
        app.session.history = board_core::History::new(&app.session.document);
        let before = app.session.document.clone();
        let profile = app.handwriting.profile.to_json().unwrap();
        let (text, standard) = if candidate {
            handwriting_standard_answer(&app, "4+1", false)
        } else {
            ("5".into(), text_kind("5"))
        };
        let generated = app
            .handwriting
            .profile
            .render_adaptive(&standard, &text, |ch| {
                crate::handwriting_fallback::sample(ch, None)
            })
            .unwrap();
        let ObjectKind::Handwritten { strokes, .. } = &generated else {
            panic!("expected generated answer");
        };
        assert!(
            strokes
                .iter()
                .map(|stroke| stroke.points.len())
                .sum::<usize>()
                > 1
        );
        let expected = standard.clone();
        let diagnostic =
            format!("{text}\n个人笔迹写入失败，已改用标准字体：无效文档：点数超过 1000000");
        app.start_math(&ctx, move || Ok((text, Some(standard))));
        if let Some(id) = &candidate_id {
            app.running_candidate = Some((id.clone(), app.ink_cache.epoch));
        }
        drain_workers(&mut app);
        let objects = &app.session.document.current_page().objects;
        let original = &before.current_page().objects;
        assert_eq!(objects.len(), original.len() + 1);
        assert_eq!(&objects[..original.len()], original.as_slice());
        let answer = objects.last().unwrap().clone();
        assert_eq!(answer.kind, expected);
        assert_eq!(app.session.document.revision, before.revision + 1);
        assert_eq!(app.status, diagnostic);
        assert_eq!(app.math_result, diagnostic);
        assert_eq!(app.handwriting.profile.to_json().unwrap(), profile);
        if let Some(id) = &candidate_id {
            let entry = app.ink_cache.get(id).unwrap();
            assert_eq!(Some(&answer.id), result_id.as_ref());
            assert_eq!(Some(&entry.result_id), result_id.as_ref());
            assert!(entry.result_present);
        }
        app.undo(false);
        assert_eq!(app.session.document.current_page().objects, *original);
        assert!(
            !app.session.history.can_undo(),
            "failed handwriting must not add history"
        );
        assert!(app.session.history.can_redo());
        if let Some(id) = &candidate_id {
            assert!(!app.ink_cache.get(id).unwrap().result_present);
        }
        app.undo(true);
        assert_eq!(
            app.session.document.current_page().objects.last(),
            Some(&answer)
        );
        assert_eq!(
            &app.session.document.current_page().objects[..original.len()],
            original.as_slice()
        );
        assert!(!app.session.history.can_redo());
        if let Some(id) = &candidate_id {
            assert!(app.ink_cache.get(id).unwrap().result_present);
        }
    }
}

#[test]
fn handwriting_full_object_budget_reports_both_insert_failures_without_success_status() {
    for candidate in [false, true] {
        let mut app = default_app();
        let ctx = egui::Context::default();
        let candidate_id = candidate.then(|| recognize_sample_at(&mut app, &ctx, Vec2::ZERO));
        let result_id = candidate_id
            .as_ref()
            .map(|id| app.ink_cache.get(id).unwrap().result_id.clone());
        let objects = &mut app.session.document.pages[0].objects;
        for index in objects.len()..board_core::MAX_DOCUMENT_OBJECTS {
            objects.push(BoardObject {
                id: format!("capacity-{index}"),
                kind: ObjectKind::Text {
                    position: Point {
                        x: 10_000.0,
                        y: 10_000.0,
                    },
                    text: "x".into(),
                    size: 24.0,
                    color: Color::default(),
                },
            });
        }
        app.session.document.validate().unwrap();
        app.session.history = board_core::History::new(&app.session.document);
        let before = app.session.document.clone();
        let profile = app.handwriting.profile.to_json().unwrap();
        assert_handwriting_answer(&handwriting_synthesized_five(&app), "5", &text_kind("5"));
        app.start_math(&ctx, || Ok(("5".into(), Some(text_kind("5")))));
        if let Some(id) = &candidate_id {
            app.running_candidate = Some((id.clone(), app.ink_cache.epoch));
        }
        drain_workers(&mut app);
        let diagnostic = "个人笔迹写入失败：无效文档：对象数量超过 100000\n标准答案写入失败：无效文档：对象数量超过 100000";
        assert_eq!(app.status, diagnostic);
        assert_eq!(app.math_result, diagnostic);
        assert_eq!(app.session.document, before);
        assert!(!app.session.history.can_undo());
        assert!(!app.session.history.can_redo());
        assert_eq!(app.handwriting.profile.to_json().unwrap(), profile);
        assert!(!app.math_worker.busy());
        assert!(app.math_ticket.is_none());
        assert!(app.running_candidate.is_none());
        if let Some(id) = &candidate_id {
            let entry = app.ink_cache.get(id).unwrap();
            assert_eq!(Some(&entry.result_id), result_id.as_ref());
            assert!(!entry.result_present);
        }
        app.poll_workers(&ctx);
        assert_eq!(
            app.status, diagnostic,
            "idle polling must retain the insertion error"
        );
        assert_eq!(app.math_result, diagnostic);
    }
}

fn handwriting_answer_profile(labels: &str) -> crate::handwriting::Profile {
    let mut profile = crate::handwriting::Profile::default();
    for label in labels.chars().filter(|ch| !ch.is_whitespace()) {
        if profile.sample_count(label) > 0 {
            continue;
        }
        // Synthetic labelled ink in the 128×128 sampling canvas, not model output.
        profile
            .add_sample(
                label,
                vec![board_core::HandwritingStroke {
                    points: vec![
                        StrokePoint {
                            x: 20.0,
                            y: 24.0,
                            time: 0.0,
                            pressure: 0.4,
                        },
                        StrokePoint {
                            x: 56.0,
                            y: 96.0,
                            time: 0.25,
                            pressure: 0.8,
                        },
                    ],
                    style: Style::default(),
                }],
            )
            .unwrap();
    }
    profile
}

fn handwriting_standard_answer(
    app: &BoardApp,
    expression: &str,
    answer_prompt: bool,
) -> (String, ObjectKind) {
    let output = features::calculation_display(expression, app.math_bounds, answer_prompt).unwrap();
    let text = output.text.clone();
    let kind = output.into_kind(Point { x: 300.0, y: 200.0 }, app.style.color);
    (text, kind)
}

fn assert_handwriting_answer(kind: &ObjectKind, semantic: &str, standard: &ObjectKind) {
    let ObjectKind::Handwritten {
        position,
        text,
        layout,
        strokes,
    } = kind
    else {
        panic!("expected a handwritten answer, got {kind:?}");
    };
    assert_eq!(text, semantic);
    match standard {
        ObjectKind::Text { position: p, .. } => {
            assert_eq!(position, p);
            assert_eq!(layout, &None);
        }
        ObjectKind::Math {
            position: p,
            layout: expected,
            ..
        } => {
            assert_eq!(position, p);
            assert_eq!(layout.as_ref(), Some(expected));
        }
        _ => panic!("expected a standard Text or Math answer"),
    }
    assert!(!strokes.is_empty());
    assert!(strokes.iter().all(|stroke| !stroke.points.is_empty()));
    BoardObject {
        id: "handwriting-answer-validation".into(),
        kind: kind.clone(),
    }
    .validate()
    .unwrap();
}

#[test]
fn handwriting_disabled_preserves_original_text_and_math_answers() {
    for expression in ["2x+3=x-1", "1/2+1/3"] {
        let mut app = app();
        assert!(!app.handwriting.enabled);
        app.handwriting.profile = handwriting_answer_profile("x=-456");
        let (text, kind) = handwriting_standard_answer(&app, expression, true);
        let expected = kind.clone();
        let expected_text = text.clone();
        app.start_math(&egui::Context::default(), move || Ok((text, Some(kind))));
        drain_workers(&mut app);
        let objects = &app.session.document.current_page().objects;
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].kind, expected);
        assert_eq!(app.math_result, expected_text);
        assert_eq!(app.status, expected_text);
    }
}

#[test]
fn handwriting_manual_start_math_is_one_semantic_object_and_history_item() {
    for expression in ["2x+3=x-1", "1/2+1/3"] {
        let mut app = app();
        app.handwriting.enabled = true;
        app.handwriting.profile = handwriting_answer_profile("x=-456");
        app.expression = expression.into();
        let (text, kind) = handwriting_standard_answer(&app, expression, true);
        let standard = kind.clone();
        let semantic = text.clone();
        app.start_math(&egui::Context::default(), move || Ok((text, Some(kind))));
        drain_workers(&mut app);
        let objects = app.session.document.current_page().objects.clone();
        assert_eq!(objects.len(), 1);
        assert_handwriting_answer(&objects[0].kind, &semantic, &standard);
        assert_eq!(app.expression, expression);
        assert_eq!(app.math_result, semantic);
        assert_eq!(app.status, semantic);
        assert_eq!(app.session.document.revision, 1);
        app.undo(false);
        assert!(app.session.document.current_page().objects.is_empty());
        app.undo(true);
        assert_eq!(app.session.document.current_page().objects, objects);
    }
}

#[test]
fn handwriting_clicked_ink_preserves_sources_and_one_shot_regeneration() {
    let mut app = app();
    app.handwriting.enabled = true;
    app.handwriting.profile = handwriting_answer_profile("=2");
    let ctx = egui::Context::default();
    let candidate_id = recognize_sample_at(&mut app, &ctx, Vec2::ZERO);
    let sources = app.session.document.current_page().objects.clone();
    assert_eq!(sources.len(), 4);
    assert!(
        sources
            .iter()
            .all(|o| matches!(o.kind, ObjectKind::Stroke { .. }))
    );
    assert!(!app.math_worker.busy());
    click_ink_icon(&mut app, &ctx);
    app.confirm_calculation(&ctx);
    drain_workers(&mut app);
    let objects = app.session.document.current_page().objects.clone();
    assert_eq!(objects.len(), sources.len() + 1);
    assert_eq!(&objects[..sources.len()], sources.as_slice());
    let answer = objects.last().unwrap().clone();
    assert!(
        matches!(&answer.kind, ObjectKind::Handwritten { text, layout: Some(layout), strokes, .. }
        if text == "= 2" && features::layout_text(layout) == "= 2" && !strokes.is_empty())
    );
    assert_eq!(
        app.ink_cache.get(&candidate_id).unwrap().result_id,
        answer.id
    );
    assert!(app.ink_cache.get(&candidate_id).unwrap().result_present);
    assert!(app.activate_candidate(&candidate_id));
    app.confirm_calculation(&ctx);
    assert!(!app.math_worker.busy());
    plot_frame(&mut app, &ctx, vec![]);
    assert!(
        app.controls.is_empty(),
        "completed candidate must hide its icon"
    );
    assert_eq!(app.session.document.current_page().objects, objects);

    app.undo(false);
    assert_eq!(app.session.document.current_page().objects, sources);
    assert!(!app.ink_cache.get(&candidate_id).unwrap().result_present);
    app.undo(true);
    assert_eq!(app.session.document.current_page().objects, objects);
    assert!(app.activate_candidate(&candidate_id));
    app.confirm_calculation(&ctx);
    assert!(!app.math_worker.busy());

    app.apply(vec![Operation::Delete {
        id: answer.id.clone(),
    }]);
    assert_eq!(app.session.document.current_page().objects, sources);
    assert!(!app.ink_cache.get(&candidate_id).unwrap().result_present);
    click_ink_icon(&mut app, &ctx);
    drain_workers(&mut app);
    assert_eq!(app.session.document.current_page().objects, objects);
    assert_eq!(
        app.session.document.current_page().objects.last(),
        Some(&answer)
    );
    app.undo(false);
    assert_eq!(app.session.document.current_page().objects, sources);
    app.undo(true);
    assert_eq!(app.session.document.current_page().objects, objects);
}

#[test]
fn handwriting_unsupported_character_without_font_falls_back_to_whole_original_answer() {
    for labels in ["", "5"] {
        let mut app = app();
        app.handwriting.enabled = true;
        app.export_resources = Default::default();
        assert!(app.export_resources.handwriting_font().is_none());
        app.handwriting.profile = handwriting_answer_profile(labels);
        let text = "5漢".to_owned();
        let kind = text_kind(&text);
        let expected = kind.clone();
        let diagnostic = format!(
            "{text}\n个人笔迹未应用，已保留标准字体：Handwriting fallback for 漢: 没有字符「漢」的基础字形；本机字体不可用"
        );
        app.start_math(&egui::Context::default(), move || Ok((text, Some(kind))));
        drain_workers(&mut app);
        let objects = app.session.document.current_page().objects.clone();
        assert_eq!(objects.len(), 1, "fallback must not insert partial ink");
        assert_eq!(objects[0].kind, expected);
        assert_eq!(app.math_result, diagnostic);
        assert_eq!(app.status, diagnostic);
        app.undo(false);
        assert!(app.session.document.current_page().objects.is_empty());
        app.undo(true);
        assert_eq!(app.session.document.current_page().objects, objects);
    }
}

#[test]
fn handwriting_move_keeps_local_ink_and_erase_removes_whole_answer() {
    let mut app = app();
    app.handwriting.enabled = true;
    app.handwriting.profile = handwriting_answer_profile("56");
    let (text, kind) = handwriting_standard_answer(&app, "1/2+1/3", true);
    let ctx = egui::Context::default();
    app.start_math(&ctx, move || Ok((text, Some(kind))));
    drain_workers(&mut app);
    let original = app.session.document.current_page().objects[0].clone();
    let mut expected = original.clone();
    editing::translate(&mut expected, Point { x: 100.0, y: 80.0 });
    match (&original.kind, &expected.kind) {
        (
            ObjectKind::Handwritten {
                position: before,
                text,
                layout,
                strokes,
            },
            ObjectKind::Handwritten {
                position: after,
                text: moved_text,
                layout: moved_layout,
                strokes: moved_strokes,
            },
        ) => {
            assert_eq!(
                *after,
                Point {
                    x: before.x + 100.0,
                    y: before.y + 80.0
                }
            );
            assert_eq!(text, moved_text);
            assert_eq!(layout, moved_layout);
            assert_eq!(strokes, moved_strokes);
        }
        _ => panic!("expected frozen handwritten ink"),
    }
    let center = board_render::object_bounds(&original).center();
    assert!(editing::hit_test(&original, center, 0.0));
    app.tool = Tool::Select;
    eraser_frame(&mut app, &ctx, vec![]);
    eraser_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(center), pointer(center, true)],
    );
    assert_eq!(app.selected.as_ref(), Some(&original.id));
    let end = center + Vec2::new(100.0, 80.0);
    eraser_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
    eraser_frame(&mut app, &ctx, vec![pointer(end, false)]);
    assert_eq!(
        app.session.document.current_page().objects,
        vec![expected.clone()]
    );
    app.undo(false);
    assert_eq!(app.session.document.current_page().objects, vec![original]);
    app.undo(true);
    assert_eq!(
        app.session.document.current_page().objects,
        vec![expected.clone()]
    );

    app.tool = Tool::Eraser;
    app.eraser = 2.0;
    eraser_frame(&mut app, &ctx, vec![]);
    eraser_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(end), pointer(end, true)],
    );
    assert!(erased_document(&app).current_page().objects.is_empty());
    assert_eq!(
        app.session.document.current_page().objects,
        vec![expected.clone()]
    );
    eraser_frame(&mut app, &ctx, vec![pointer(end, false)]);
    assert!(app.session.document.current_page().objects.is_empty());
    app.undo(false);
    assert_eq!(app.session.document.current_page().objects, vec![expected]);
    app.undo(true);
    assert!(app.session.document.current_page().objects.is_empty());
}

#[test]
fn handwriting_stale_or_cancelled_worker_never_writes_or_overwrites_status() {
    for change in 0..6 {
        let mut app = app();
        app.handwriting.enabled = true;
        app.handwriting.profile = handwriting_answer_profile("56");
        let (text, kind) = handwriting_standard_answer(&app, "1/2+1/3", true);
        let (tx, rx) = mpsc::channel();
        app.start_math(&egui::Context::default(), move || {
            rx.recv_timeout(Duration::from_secs(3)).unwrap();
            Ok((text, Some(kind)))
        });
        assert!(app.math_worker.busy());
        match change {
            0 => app.cancel_math(),
            1 => app.cancel_ink(),
            2 => app.expression = "new input".into(),
            3 => app.session.document.revision += 1,
            4 => {
                app.session.document.add_page().unwrap();
            }
            _ => app.session.document.id = new_id(),
        }
        let document = app.session.document.clone();
        app.math_result = "new math status".into();
        app.status = "new board status".into();
        tx.send(()).unwrap();
        drain_workers(&mut app);
        assert_eq!(app.session.document, document, "change {change}");
        assert_eq!(app.math_result, "new math status", "change {change}");
        assert_eq!(app.status, "new board status", "change {change}");
        assert!(app.math_ticket.is_none(), "change {change}");
    }
}

fn handwriting_reveal_panel_text(
    app: &mut BoardApp,
    ctx: &egui::Context,
    text: &str,
    scroll_pos: Pos2,
) {
    fn text_rect(shape: &egui::Shape, text: &str) -> Option<Rect> {
        match shape {
            egui::Shape::Text(t) if t.galley.text() == text => {
                Some(Rect::from_min_size(t.pos, t.galley.size()))
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| text_rect(s, text)),
            _ => None,
        }
    }
    for _ in 0..20 {
        let output = panel_frame(app, ctx, vec![]);
        if output.shapes.iter().any(|shape| {
            text_rect(&shape.shape, text).is_some_and(|rect| {
                shape
                    .clip_rect
                    .intersect(ctx.content_rect())
                    .contains_rect(rect.expand(4.0))
            })
        }) {
            return;
        }
        panel_frame(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(scroll_pos),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, -160.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: Default::default(),
                },
            ],
        );
        // Let smooth scrolling settle before locating or clicking the next control.
        for _ in 0..30 {
            panel_frame(app, ctx, vec![]);
        }
    }
    panic!("handwriting panel control not reachable by scrolling: {text}");
}

#[test]
fn handwriting_settings_ui_cancels_pending_answer() {
    for clear_profile in [false, true] {
        let mut app = app();
        app.math = true;
        app.handwriting.enabled = true;
        app.handwriting.profile = handwriting_answer_profile("56");
        let ctx = egui::Context::default();
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
        }
        let expanded = click_panel_text(&mut app, &ctx, "个人笔迹答案（实验性）");
        let scroll_pos = text_position(&expanded, "自动学习并使用个人笔迹（默认开启）");
        if clear_profile {
            handwriting_reveal_panel_text(
                &mut app,
                &ctx,
                "确认清空整个个人笔迹档案（含风格统计）",
                scroll_pos,
            );
        }
        let (text, kind) = handwriting_standard_answer(&app, "1/2+1/3", true);
        let (tx, rx) = mpsc::channel();
        app.start_math(&ctx, move || {
            rx.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok((text, Some(kind)))
        });
        assert!(app.math_ticket.is_some());
        if clear_profile {
            click_panel_text(&mut app, &ctx, "确认清空整个个人笔迹档案（含风格统计）");
            assert!(
                app.math_ticket.is_some(),
                "confirmation alone must not cancel"
            );
            handwriting_reveal_panel_text(&mut app, &ctx, "清空全部样本与风格", scroll_pos);
            click_panel_text(&mut app, &ctx, "清空全部样本与风格");
            assert_eq!(app.handwriting.profile.counts(), (0, 0));
        } else {
            click_panel_text(&mut app, &ctx, "自动学习并使用个人笔迹（默认开启）");
            assert!(!app.handwriting.enabled);
        }
        assert!(app.math_ticket.is_none());
        let document = app.session.document.clone();
        app.math_result = "settings changed".into();
        tx.send(()).unwrap();
        drain_workers(&mut app);
        assert_eq!(app.session.document, document);
        assert_eq!(app.math_result, "settings changed");
    }
}

#[test]
fn handwriting_worker_uses_profile_snapshot_and_saved_answer_needs_no_profile() {
    let mut original_app = app();
    original_app.handwriting.enabled = true;
    original_app.handwriting.profile = handwriting_answer_profile("56");
    let (text, kind) = handwriting_standard_answer(&original_app, "1/2+1/3", true);
    let expected = original_app
        .handwriting
        .profile
        .render(&kind, &text)
        .unwrap();
    let ctx = egui::Context::default();
    let (tx, rx) = mpsc::channel();
    original_app.start_math(&ctx, move || {
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        Ok((text, Some(kind)))
    });
    // Deliberately bypass UI cancellation to verify the worker owns a frozen clone.
    original_app.handwriting.profile.clear();
    tx.send(()).unwrap();
    drain_workers(&mut original_app);
    assert_eq!(
        original_app.session.document.current_page().objects.len(),
        1
    );
    assert_eq!(
        original_app.session.document.current_page().objects[0].kind,
        expected
    );
    let document = original_app.session.document.clone();
    let path = std::env::temp_dir().join(format!("handwriting-answer-{}.neoboard", new_id()));
    original_app.session.save_document(&path).unwrap();
    let mut reopened = app();
    assert_eq!(reopened.handwriting.profile.counts(), (0, 0));
    let result = reopened.session.open_document(&path, false);
    std::fs::remove_file(&path).unwrap();
    result.unwrap();
    assert_eq!(reopened.session.document, document);

    original_app.handwriting.profile = handwriting_answer_profile("x");
    original_app.handwriting.profile.clear();
    eraser_frame(&mut original_app, &ctx, vec![]);
    eraser_frame(&mut reopened, &egui::Context::default(), vec![]);
    assert_eq!(original_app.session.document, document);
    assert_eq!(reopened.session.document, document);
    original_app.undo(false);
    assert!(
        original_app
            .session
            .document
            .current_page()
            .objects
            .is_empty()
    );
    original_app.undo(true);
    assert_eq!(
        original_app.session.document.current_page().objects,
        document.current_page().objects
    );
}

#[test]
fn handwriting_selected_answer_displays_and_copies_standard_semantics() {
    let mut app = app();
    app.handwriting.enabled = true;
    app.handwriting.profile = handwriting_answer_profile("56");
    let (text, kind) = handwriting_standard_answer(&app, "1/2+1/3", true);
    let semantic = text.clone();
    let ctx = egui::Context::default();
    app.start_math(&ctx, move || Ok((text, Some(kind))));
    drain_workers(&mut app);
    app.selected = Some(app.session.document.current_page().objects[0].id.clone());
    app.math = true;
    // Keep the status distinct so finding this text proves the selected-object display.
    app.math_result.clear();
    let document = app.session.document.clone();
    panel_frame(&mut app, &ctx, vec![]);
    let output = panel_frame(&mut app, &ctx, vec![]);
    text_position(&output, "所选手写答案的标准文字：");
    text_position(&output, &semantic);
    let pos = text_position(&output, "复制所选答案标准文字");
    panel_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
    );
    // Inspect egui's output command; never access the operating-system clipboard.
    let copied = panel_frame(&mut app, &ctx, vec![pointer(pos, false)]);
    assert!(copied.platform_output.commands.iter().any(
        |command| matches!(command, egui::OutputCommand::CopyText(text) if text == &semantic)
    ));
    assert_eq!(app.session.document, document);
    assert!(!app.math_worker.busy());
}
