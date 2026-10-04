// #region debug-point C:backend-tests
#[test]
fn debug_backend_accepts_only_explicit_debug_whitelist() {
    for (name, expected) in [
        ("dx12", eframe::wgpu::Backends::DX12),
        ("vulkan", eframe::wgpu::Backends::VULKAN),
    ] {
        assert_eq!(
            debug_ink::backend_from_env(Some("1"), Some(name)),
            Some(expected),
        );
        for debug in [
            None,
            Some(""),
            Some("0"),
            Some("true"),
            Some(" 1"),
            Some("1 "),
        ] {
            assert_eq!(debug_ink::backend_from_env(debug, Some(name)), None);
        }
    }
}

#[test]
fn debug_backend_missing_or_invalid_preserves_default() {
    for backend in [
        None,
        Some(""),
        Some("DX12"),
        Some("Vulkan"),
        Some(" dx12"),
        Some("vulkan "),
        Some("dx12,vulkan"),
        Some("gl"),
        Some("metal"),
        Some("default"),
    ] {
        assert_eq!(debug_ink::backend_from_env(Some("1"), backend), None);
        assert_eq!(debug_ink::backend_from_env(None, backend), None);
    }
}
// #endregion

#[test]
fn debug_ink_is_disabled_outside_test_capture() {
    assert!(!debug_ink::enabled());
    assert!(debug_ink::start().is_none());
    assert_eq!(debug_ink::micros(None), 0);
    assert_eq!(debug_ink::frame_gap(), 0);
    debug_ink::event("B", "test", serde_json::Value::Null);
    debug_ink::sample(0, "B", "test", ["preview"; 4], [1; 4], || {
        panic!("disabled sampling must not build diagnostic data")
    });
}

#[test]
fn debug_ink_capture_preserves_stage_and_canvas_metrics() {
    debug_ink::capture_begin();
    assert!(debug_ink::enabled());
    assert!(debug_ink::start().is_some());
    {
        let _stage = debug_ink::Stage::begin(4, "candidate_cache_sync");
    }
    for _ in 0..2050 {
        debug_ink::sample(
            0,
            "B",
            "test",
            ["preview", "page_clone_apply", "textures", "render"],
            [11, 22, 33, 44],
            || panic!("test capture must not build diagnostic data"),
        );
    }
    debug_ink::event("B", "test", serde_json::Value::Null);
    let stages = debug_ink::capture_end();
    assert_eq!(stages["candidate_cache_sync"]["n"], 1);
    for (name, value) in [
        ("canvas_preview", 11),
        ("canvas_page_clone_apply", 22),
        ("canvas_textures", 33),
        ("canvas_render", 44),
    ] {
        assert_eq!(stages[name]["n"], 2048);
        assert_eq!(stages[name]["p50_us"], value);
        assert_eq!(stages[name]["max_us"], value);
    }
    assert!(!debug_ink::enabled());
    assert!(debug_ink::start().is_none());
    assert_eq!(debug_ink::frame_gap(), 0);
}

fn fullscreen_ink_fixture(count: usize) -> board_core::Document {
    let mut document = board_core::Document::new();
    let mut seed = 0x5eed_b001_u64;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((seed >> 32) as u32) as f32 / u32::MAX as f32
    };
    document.pages[0].objects = (0..count)
        .map(|i| {
            let x = 30.0 + next() * 1730.0;
            let y = 40.0 + next() * 880.0;
            let phase = next() * std::f32::consts::TAU;
            BoardObject {
                id: format!("bench-{i}"),
                kind: ObjectKind::Stroke {
                    points: (0..128)
                        .map(|j| StrokePoint {
                            x: x + j as f32 * 0.8,
                            y: y + (j as f32 * 0.08 + phase).sin() * 18.0,
                            time: j as f64 / 120.0,
                            pressure: 1.0,
                        })
                        .collect(),
                    style: Style::default(),
                },
            }
        })
        .collect();
    document
}

#[test]
fn native_frame_distribution_reports_quantiles_and_budget() {
    let stats = debug_ink::distribution(vec![6061, 1, 6060, 10000]);
    assert_eq!(stats["p50_us"], 6060);
    assert_eq!(stats["p95_us"], 10000);
    assert_eq!(stats["p99_us"], 10000);
    assert_eq!(stats["max_us"], 10000);
    assert_eq!(stats["over_budget_165hz"], 2);
    assert_eq!(debug_ink::distribution(vec![])["n"], 0);
}

#[test]
#[ignore = "explicit NEO_PERF_FIXTURE_PATH under repository .dbg only; writes synthetic document, no GUI"]
fn write_native_fullscreen_ink_fixture() -> std::io::Result<()> {
    use std::{fs, io};
    let path = PathBuf::from(std::env::var_os("NEO_PERF_FIXTURE_PATH").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "set NEO_PERF_FIXTURE_PATH explicitly",
        )
    })?);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let allowed = root.join(".dbg").canonicalize()?;
    if allowed.parent() != Some(root.as_path())
        || !path.is_absolute()
        || path
            .parent()
            .map(|p| p.canonicalize())
            .transpose()?
            .as_ref()
            != Some(&allowed)
        || path.extension().and_then(|s| s.to_str()) != Some("neoboard")
        || path
            .file_name()
            .is_none_or(|name| name.to_string_lossy().contains(':'))
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "fixture must be an absolute .neoboard path directly under repository .dbg",
        ));
    }
    // Reserve without following/replacing an existing file, including a symlink.
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let staging = allowed.join(format!("fixture-stage-{}", new_id()));
    if let Err(error) = fs::create_dir(&staging) {
        drop(output);
        let _ = fs::remove_file(&path);
        return Err(error);
    }
    let result =
        (|| -> io::Result<()> {
            let temporary = staging.join("synthetic.neoboard");
            let mut session = Session::new(board_session::AppKind::Drawing);
            session.document = fullscreen_ink_fixture(1500);
            session.history = board_core::History::new(&session.document);
            session
                .save_document(&temporary)
                .map_err(|e| io::Error::other(e.message))?;
            let mut saved = fs::File::open(&temporary)?;
            io::copy(&mut saved, &mut output)?;
            output.sync_all()?;
            let mut reopened = Session::new(board_session::AppKind::Drawing);
            reopened
                .open_document(&path, false)
                .map_err(|e| io::Error::other(e.message))?;
            assert_eq!(reopened.document.current_page().objects.len(), 1500);
            assert!(reopened.document.current_page().objects.iter().all(
                |o| matches!(&o.kind, ObjectKind::Stroke { points, .. } if points.len() == 128)
            ));
            Ok(())
        })();
    drop(output);
    if staging.is_dir() {
        fs::remove_dir_all(&staging)?;
    }
    if result.is_err() {
        let _ = fs::remove_file(&path);
    }
    result?;
    eprintln!(
        "synthetic 1500x128 fixture saved: {} (1920x1080 logical reference; no GUI/present measurement)",
        path.display()
    );
    Ok(())
}

// #region debug-point B:fullscreen-baseline
#[test]
#[ignore = "release-only synthetic egui benchmark; reports to stderr; no native window"]
fn fullscreen_ink_performance_baseline() {
    if cfg!(debug_assertions) {
        panic!("run with --release");
    }
    const SEED: u64 = 0x5eed_b001;
    fn report(count: usize, phase: &str, samples: Vec<u64>, extra: serde_json::Value) {
        let stages = debug_ink::capture_end();
        debug_ink::report_benchmark(serde_json::json!({
            "phase": phase, "strokes": count, "points_per_stroke": 128,
            "seed": SEED, "screen": [1920, 1080], "pixels_per_point": 1,
            "profile": "release", "cpu_only": true, "includes_tessellation": !phase.starts_with("candidate_cache"),
            "strokes_is_initial_count": true,
            "timing": debug_ink::distribution(samples), "stages": stages, "extra": extra,
        }));
    }
    fn run_frame(app: &mut BoardApp, ctx: &egui::Context, events: Vec<egui::Event>) -> u64 {
        let start = Instant::now();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1920.0, 1080.0))),
                events,
                ..Default::default()
            },
            |ui| app.board_ui(ui, ctx),
        );
        let tessellate = Instant::now();
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        debug_ink::record(
            11,
            "egui_tessellate",
            tessellate.elapsed().as_micros() as u64,
        );
        std::hint::black_box(&primitives);
        let elapsed = start.elapsed().as_micros() as u64;
        drop(primitives);
        elapsed
    }
    for count in [100, 500, 1500] {
        let mut app = app();
        app.session.document = fullscreen_ink_fixture(count);
        app.session.history = board_core::History::new(&app.session.document);
        let ctx = egui::Context::default();
        debug_ink::capture_begin();
        for _ in 0..4 {
            run_frame(&mut app, &ctx, vec![]);
        }
        debug_ink::capture_begin();
        let samples = (0..24).map(|_| run_frame(&mut app, &ctx, vec![])).collect();
        report(count, "idle", samples, serde_json::json!({"warmup": 4}));

        debug_ink::capture_begin();
        for i in 0..4 {
            run_frame(
                &mut app,
                &ctx,
                vec![egui::Event::PointerMoved(Pos2::new(
                    300.0 + i as f32 * 2.0,
                    300.0,
                ))],
            );
        }
        debug_ink::capture_begin();
        let samples = (0..24)
            .map(|i| {
                run_frame(
                    &mut app,
                    &ctx,
                    vec![egui::Event::PointerMoved(Pos2::new(
                        320.0 + i as f32 * 2.0,
                        300.0,
                    ))],
                )
            })
            .collect();
        report(
            count,
            "pointer_moves",
            samples,
            serde_json::json!({"warmup": 4}),
        );

        let start = Pos2::new(100.0, 400.0);
        debug_ink::capture_begin();
        run_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(start), pointer(start, true)],
        );
        run_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(start + Vec2::new(10.0, 0.0))],
        );
        assert!(app.gesture.is_some());
        app.gesture.as_mut().unwrap().points = (0..1024)
            .map(|j| StrokePoint {
                x: 100.0 + j as f32,
                y: 400.0 + (j as f32 * 0.04).sin() * 35.0,
                time: j as f64 / 120.0,
                pressure: 1.0,
            })
            .collect();
        for i in 0..4 {
            run_frame(
                &mut app,
                &ctx,
                vec![egui::Event::PointerMoved(Pos2::new(
                    1124.0 + i as f32 * 2.0,
                    400.0,
                ))],
            );
        }
        debug_ink::capture_begin();
        let samples = (0..24)
            .map(|i| {
                run_frame(
                    &mut app,
                    &ctx,
                    vec![egui::Event::PointerMoved(Pos2::new(
                        1132.0 + i as f32 * 2.0,
                        400.0,
                    ))],
                )
            })
            .collect();
        report(
            count,
            "long_stroke_drag",
            samples,
            serde_json::json!({"warmup": 4, "initial_gesture_points": 1024}),
        );
        debug_ink::capture_begin();
        let sample = run_frame(
            &mut app,
            &ctx,
            vec![pointer(Pos2::new(1178.0, 400.0), false)],
        );
        assert!(app.gesture.is_none());
        report(
            count,
            "long_stroke_penup",
            vec![sample],
            serde_json::json!({"single_event": true}),
        );

        let mut samples = Vec::new();
        let mut stages = Vec::new();
        for iteration in 0..14 {
            debug_ink::capture_begin();
            run_frame(
                &mut app,
                &ctx,
                vec![egui::Event::PointerMoved(start), pointer(start, true)],
            );
            run_frame(
                &mut app,
                &ctx,
                vec![egui::Event::PointerMoved(start + Vec2::new(10.0, 0.0))],
            );
            app.gesture.as_mut().unwrap().points = (0..128)
                .map(|j| StrokePoint {
                    x: 100.0 + j as f32,
                    y: 400.0 + (j as f32 * 0.08).sin() * 18.0,
                    time: j as f64 / 120.0,
                    pressure: 1.0,
                })
                .collect();
            debug_ink::capture_begin();
            let us = run_frame(
                &mut app,
                &ctx,
                vec![pointer(Pos2::new(227.0, 400.0), false)],
            );
            if iteration >= 2 {
                samples.push(us);
                stages.push(debug_ink::capture_end());
            }
        }
        debug_ink::capture_begin();
        report(
            count,
            "penup_128_points",
            samples,
            serde_json::json!({"warmup": 2, "event_stages": stages}),
        );

        app.tool = Tool::Eraser;
        app.eraser = 8.0;
        debug_ink::capture_begin();
        let erase = Pos2::new(400.0, 450.0);
        run_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(erase), pointer(erase, true)],
        );
        for i in 1..=3 {
            run_frame(
                &mut app,
                &ctx,
                vec![egui::Event::PointerMoved(
                    erase + Vec2::new(i as f32 * 4.0, 0.0),
                )],
            );
        }
        debug_ink::capture_begin();
        let samples = (4..20)
            .map(|i| {
                run_frame(
                    &mut app,
                    &ctx,
                    vec![egui::Event::PointerMoved(
                        erase + Vec2::new(i as f32 * 4.0, 0.0),
                    )],
                )
            })
            .collect();
        assert!(
            app.gesture
                .as_ref()
                .unwrap()
                .erasing
                .as_ref()
                .unwrap()
                .result
                .as_ref()
                .unwrap()
                .is_ok()
        );
        report(
            count,
            "erase_preview_continuous",
            samples,
            serde_json::json!({"warmup": 3, "max_path_points": 20, "no_commit": true}),
        );
        app.gesture = None;
        debug_ink::capture_begin();
        run_frame(
            &mut app,
            &ctx,
            vec![pointer(erase + Vec2::new(76.0, 0.0), false)],
        );

        for i in 0..8 {
            let source = app.session.document.current_page().objects[i].clone();
            let ObjectKind::Stroke { points, .. } = &source.kind else {
                unreachable!()
            };
            let context = ContextToken::capture(&app.session.document);
            let now = Instant::now();
            let mut gate = AutoCalculate::new();
            gate.set_enabled(true);
            gate.strokes_finished(now, context.clone(), std::slice::from_ref(points))
                .unwrap();
            let request = gate
                .poll(now + AutoCalculate::IDLE_DELAY, &context)
                .unwrap();
            assert!(
                app.ink_cache
                    .insert(
                        &app.session.document,
                        request,
                        vec![source],
                        None,
                        None,
                        HwrBackend::Template
                    )
                    .is_some()
            );
        }
        for phase in [
            "candidate_cache_revision_hit",
            "candidate_cache_local_validate",
        ] {
            debug_ink::capture_begin();
            for _ in 0..3 {
                if phase.ends_with("validate") {
                    app.session.document.revision += 1;
                }
                app.sync_ink_cache();
            }
            debug_ink::capture_begin();
            let samples = (0..16)
                .map(|_| {
                    if phase.ends_with("validate") {
                        app.session.document.revision += 1;
                    }
                    let start = Instant::now();
                    app.sync_ink_cache();
                    start.elapsed().as_micros() as u64
                })
                .collect();
            assert_eq!(app.ink_cache.entries.len(), 8);
            report(
                count,
                phase,
                samples,
                serde_json::json!({"warmup": 3, "candidates": 8, "unchanged_objects_new_revision": phase.ends_with("validate")}),
            );
        }
        for phase in ["thumbnails_cached", "thumbnails_revision_changed"] {
            let mut samples = Vec::new();
            debug_ink::capture_begin();
            for iteration in 0..15 {
                if phase.ends_with("changed") {
                    app.session.document.revision += 1;
                }
                if iteration == 3 {
                    debug_ink::capture_begin();
                }
                let start = Instant::now();
                let output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            Pos2::ZERO,
                            Vec2::new(1920.0, 1080.0),
                        )),
                        ..Default::default()
                    },
                    |ui| app.pages_menu(ui),
                );
                std::hint::black_box(ctx.tessellate(output.shapes, output.pixels_per_point));
                if iteration >= 3 {
                    samples.push(start.elapsed().as_micros() as u64);
                }
            }
            assert!(!app.thumbnails.is_empty());
            report(
                count,
                phase,
                samples,
                serde_json::json!({"warmup": 3, "visible_pages": 1, "isolated_menu": true}),
            );
        }
    }
}
// #endregion
