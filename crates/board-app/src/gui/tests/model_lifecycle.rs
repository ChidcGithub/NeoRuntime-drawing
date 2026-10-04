// Each fixture owns only its unique temp directory; never changes the process cwd.
struct ModelSuggestionFixture(PathBuf);

impl ModelSuggestionFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("board-model-suggestion-{}", new_id()));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn base(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn fp32(&self, base: &Path) -> PathBuf {
        let dir = base.join("models").join("texteller");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn int8(&self, base: &Path, missing: Option<&str>) -> PathBuf {
        let dir = base.join("models").join("texteller-int8");
        std::fs::create_dir_all(&dir).unwrap();
        for name in INT8_MODEL_SUGGESTION_FILES {
            if Some(name) != missing {
                // Empty files deliberately prove that suggestion does not load/validate.
                std::fs::write(dir.join(name), []).unwrap();
            }
        }
        dir
    }
}

impl Drop for ModelSuggestionFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn model_lifecycle_suggestion_prefers_complete_int8_before_fp32() {
    let fixture = ModelSuggestionFixture::new();
    let cwd = fixture.base("cwd");
    let exe = fixture.base("exe");
    fixture.fp32(&cwd);
    fixture.fp32(&exe);
    let exe_int8 = fixture.int8(&exe, None);
    assert_eq!(suggested_model_dir(Some(&cwd), Some(&exe)), Some(exe_int8));
    let cwd_int8 = fixture.int8(&cwd, None);
    assert_eq!(suggested_model_dir(Some(&cwd), Some(&exe)), Some(cwd_int8));
}

#[test]
fn model_lifecycle_suggestion_each_missing_int8_file_falls_back() {
    for missing in INT8_MODEL_SUGGESTION_FILES {
        let fixture = ModelSuggestionFixture::new();
        let cwd = fixture.base("cwd");
        let exe = fixture.base("exe");
        let fp32 = fixture.fp32(&cwd);
        fixture.int8(&cwd, Some(missing));
        fixture.int8(&exe, Some(missing));
        assert_eq!(
            suggested_model_dir(Some(&cwd), Some(&exe)),
            Some(fp32),
            "missing {missing} must prevent INT8 suggestion"
        );
        let complete = fixture.int8(&exe, None);
        assert_eq!(suggested_model_dir(Some(&cwd), Some(&exe)), Some(complete));
    }
}

#[test]
fn model_lifecycle_suggestion_preserves_cwd_exedir_and_absent_fallback_order() {
    let fixture = ModelSuggestionFixture::new();
    let cwd = fixture.base("cwd");
    let exe = fixture.base("exe");
    let cwd_default = cwd.join("models").join("texteller");
    let exe_default = exe.join("models").join("texteller");
    assert_eq!(suggested_model_dir(None, None), None);
    assert_eq!(
        suggested_model_dir(Some(&cwd), None),
        Some(cwd_default.clone())
    );
    assert_eq!(
        suggested_model_dir(None, Some(&exe)),
        Some(exe_default.clone())
    );
    assert_eq!(
        suggested_model_dir(Some(&cwd), Some(&exe)),
        Some(cwd_default.clone())
    );
    fixture.fp32(&exe);
    assert_eq!(
        suggested_model_dir(Some(&cwd), Some(&exe)),
        Some(exe_default)
    );
    fixture.fp32(&cwd);
    assert_eq!(
        suggested_model_dir(Some(&cwd), Some(&exe)),
        Some(cwd_default)
    );
    let exe_int8 = fixture.int8(&exe, None);
    assert_eq!(suggested_model_dir(None, Some(&exe)), Some(exe_int8));
    let cwd_int8 = fixture.int8(&cwd, None);
    assert_eq!(suggested_model_dir(Some(&cwd), None), Some(cwd_int8));
}

// Fake payloads exercise the same loader lifecycle without constructing an ORT session.
struct FakeModelSlot {
    loader: features::Background<std::result::Result<Arc<()>, String>>,
    model: Option<Arc<()>>,
    loaded_dir: String,
    model_dir: String,
    backend: HwrBackend,
    status: String,
}

impl FakeModelSlot {
    fn new() -> Self {
        Self {
            loader: Default::default(),
            model: None,
            loaded_dir: "model-a".into(),
            model_dir: "model-a".into(),
            backend: HwrBackend::TexTeller,
            status: "loading".into(),
        }
    }

    fn poll(&mut self) {
        poll_model_load(
            &mut self.loader,
            &mut self.model,
            &mut self.loaded_dir,
            &self.model_dir,
            self.backend,
            &mut self.status,
        );
    }

    fn drain(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.loader.busy() {
            self.poll();
            assert!(Instant::now() < deadline, "fake loader did not drain");
            std::thread::yield_now();
        }
    }

    fn unload(&mut self) {
        release_model(&mut self.loader, &mut self.model, &mut self.loaded_dir);
        self.status = "unloaded".into();
    }
}

#[test]
fn model_lifecycle_reload_drops_old_before_constructing_new() {
    let ctx = egui::Context::default();
    let mut slot = FakeModelSlot::new();
    slot.model = Some(Arc::new(()));
    let old = Arc::downgrade(slot.model.as_ref().unwrap());
    start_model_load(&mut slot.loader, &mut slot.model, &ctx, move || {
        assert!(old.upgrade().is_none(), "old model survived into new load");
        Ok(Arc::new(()))
    });
    assert!(slot.model.is_none());
    slot.drain();
    assert!(slot.model.is_some());
    assert_eq!(slot.loaded_dir, "model-a");
    assert!(slot.status.contains("CPU"));
}

#[test]
fn model_lifecycle_stale_success_and_failure_never_install_or_report_old_error() {
    for backend_changed in [false, true] {
        for failed in [false, true] {
            let mut slot = FakeModelSlot::new();
            let ctx = egui::Context::default();
            let model = Arc::new(());
            let weak = Arc::downgrade(&model);
            let (tx, rx) = mpsc::channel();
            slot.loader.start(ctx, move || {
                rx.recv().unwrap();
                if failed {
                    Err("stale failure".into())
                } else {
                    Ok(model)
                }
            });
            if backend_changed {
                slot.backend = HwrBackend::Template;
            } else {
                slot.model_dir = "model-b".into();
            }
            tx.send(()).unwrap();
            slot.drain();
            assert!(slot.model.is_none());
            assert!(weak.upgrade().is_none());
            assert!(slot.loaded_dir.is_empty());
            assert!(slot.status.contains("过期"));
            assert!(!slot.status.contains("stale failure"));
        }
    }
}

#[test]
fn model_lifecycle_failure_and_panic_clear_loaded_directory() {
    for panics in [false, true] {
        let mut slot = FakeModelSlot::new();
        slot.loader.start(egui::Context::default(), move || {
            assert!(!panics, "fake loader panic");
            Err("fake load failure".into())
        });
        slot.drain();
        assert!(slot.model.is_none());
        assert!(slot.loaded_dir.is_empty());
        assert!(slot.status.contains("模型加载失败"));
        assert!(slot.status.contains(if panics {
            "异常"
        } else {
            "fake load failure"
        }));
    }
}

#[test]
fn model_lifecycle_unload_cancels_but_keeps_slot_until_payload_is_dropped() {
    let mut slot = FakeModelSlot::new();
    let ctx = egui::Context::default();
    let pending = Arc::new(());
    let weak = Arc::downgrade(&pending);
    let (tx, rx) = mpsc::channel();
    slot.loader.start(ctx.clone(), move || {
        rx.recv().unwrap();
        Ok(pending)
    });
    slot.unload();
    slot.poll();
    assert!(slot.loader.busy());
    assert!(slot.loaded_dir.is_empty());
    assert!(weak.upgrade().is_some());
    slot.loader
        .start(ctx.clone(), || panic!("must not start while draining"));
    tx.send(()).unwrap();
    slot.drain();
    assert!(weak.upgrade().is_none());
    assert!(slot.model.is_none());
    assert_eq!(slot.status, "unloaded");

    slot.loaded_dir = "model-a".into();
    slot.loader.start(ctx, || Ok(Arc::new(())));
    slot.drain();
    assert!(slot.model.is_some(), "cancelled slot must be reusable");
    let installed = Arc::downgrade(slot.model.as_ref().unwrap());
    slot.unload();
    assert!(installed.upgrade().is_none());
    assert!(slot.loaded_dir.is_empty());
}

#[test]
fn model_lifecycle_reload_rejects_worker_arc_until_drain_and_worker_is_reusable() {
    let mut app = app();
    app.set_hwr_backend(HwrBackend::TexTeller);
    app.set_ink_math_mode(InkMathMode::Confirm);
    // Absolute but never passed to the real loader: the worker guard must win.
    app.model_dir = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let ctx = egui::Context::default();
    let now = Instant::now();
    write_sample(&mut app, now);
    let fake_model = Arc::new(());
    let weak = Arc::downgrade(&fake_model);
    let (tx, rx) = mpsc::channel();
    app.poll_ink_result(&ctx, now + AutoCalculate::IDLE_DELAY, move |_| {
        rx.recv().unwrap();
        drop(fake_model);
        mock_neural("1+1", true, -0.1)
    });
    assert!(!app.model_load_ready());
    app.load_model(&ctx);
    assert!(!app.model_loader.busy());
    assert!(app.hwr_worker.busy());
    assert!(weak.upgrade().is_some());
    assert!(app.model_status.contains("排空"));
    tx.send(()).unwrap();
    drain_workers(&mut app);
    assert!(weak.upgrade().is_none());
    assert!(app.recognition.is_none());
    assert!(app.model_load_ready());

    let now = Instant::now();
    write_sample(&mut app, now);
    app.poll_ink_result(&ctx, now + AutoCalculate::IDLE_DELAY, |_| {
        mock_neural("1+1", true, -0.1)
    });
    drain_workers(&mut app);
    assert!(
        app.recognition.is_some(),
        "cancel must not lose worker reuse"
    );
}

#[test]
fn model_lifecycle_template_or_unload_cancels_loader_even_after_switching_back() {
    for template in [false, true] {
        let mut app = app();
        let ctx = egui::Context::default();
        app.set_hwr_backend(HwrBackend::TexTeller);
        app.loaded_model_dir = app.model_dir.trim().to_owned();
        let (tx, rx) = mpsc::channel();
        app.model_loader.start(ctx.clone(), move || {
            rx.recv().unwrap();
            Err("cancelled load error".into())
        });
        if template {
            app.set_hwr_backend(HwrBackend::Template);
            app.set_hwr_backend(HwrBackend::TexTeller);
        } else {
            app.unload_model();
        }
        assert!(app.loaded_model_dir.is_empty());
        assert!(!app.model_load_ready());
        app.load_model(&ctx);
        app.poll_workers(&ctx);
        assert!(app.model_loader.busy());
        tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while app.model_loader.busy() {
            app.poll_workers(&ctx);
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(app.model_load_ready());
        assert!(!app.model_status.contains("cancelled load error"));
        assert!(app.neural_recognizer.is_none());
        assert!(app.loaded_model_dir.is_empty());
    }
}

#[test]
fn model_lifecycle_off_retains_model_context_and_loader_failure_is_visible() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.set_hwr_backend(HwrBackend::TexTeller);
    app.set_ink_math_mode(InkMathMode::Confirm);
    app.loaded_model_dir = app.model_dir.trim().to_owned();
    let loaded_dir = app.loaded_model_dir.clone();
    let (tx, rx) = mpsc::channel();
    app.model_loader.start(ctx.clone(), move || {
        rx.recv().unwrap();
        Err("current load failure".into())
    });
    app.set_ink_math_mode(InkMathMode::Off);
    assert_eq!(app.loaded_model_dir, loaded_dir);
    tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while app.model_loader.busy() {
        app.poll_workers(&ctx);
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(app.model_status.contains("current load failure"));
    assert!(app.loaded_model_dir.is_empty());
}

#[test]
fn model_lifecycle_unload_button_cancels_loading_without_native_gui() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.math = true;
    app.set_hwr_backend(HwrBackend::TexTeller);
    app.loaded_model_dir = app.model_dir.trim().to_owned();
    let (tx, rx) = mpsc::channel();
    app.model_loader.start(ctx.clone(), move || {
        rx.recv().unwrap();
        Err("cancelled UI load".into())
    });
    let panel_frame = |app: &mut BoardApp, events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 900.0))),
                events,
                ..Default::default()
            },
            |_| app.panels(&ctx),
        );
        output.textures_delta.clear();
        output
    };
    panel_frame(&mut app, vec![]);
    let output = panel_frame(&mut app, vec![]);
    let pos = text_position(&output, "卸载模型");
    panel_frame(
        &mut app,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
    );
    panel_frame(&mut app, vec![pointer(pos, false)]);
    assert!(app.loaded_model_dir.is_empty());
    assert!(app.model_loader.busy());
    assert!(!app.model_load_ready());
    tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while app.model_loader.busy() {
        app.poll_workers(&ctx);
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(!app.model_status.contains("cancelled UI load"));
}
