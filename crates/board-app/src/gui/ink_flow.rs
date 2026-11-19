// 墨迹/数学工作流与文档操作：后台轮询、候选缓存、变更应用与提交。从 gui.rs 拆出（纯移动）。
use super::*;

impl BoardApp {
    pub(super) fn poll_backend_ink(&mut self, ctx: &egui::Context, now: Instant) {
        match self.hwr_backend {
            HwrBackend::Template => {
                let recognizer = self.recognizer.clone();
                self.poll_ink(ctx, now, move |strokes| {
                    recognizer.recognize(strokes).map_err(|e| e.to_string())
                });
            }
            HwrBackend::TexTeller => {
                let Some(model) = self.neural_recognizer.clone() else {
                    return;
                };
                if self.model_loader.busy() || self.model_dir.trim() != self.loaded_model_dir {
                    return;
                }
                self.poll_ink_result(ctx, now, move |strokes| {
                    let Ok(mut model) = model.try_lock() else {
                        return Err("模型忙或锁异常，请稍后重新书写".to_string()).into();
                    };
                    let (result, preview) = model.recognize_with_preview(strokes);
                    let mut result = match result {
                        Ok(raw) => neural_result(raw),
                        Err(error) => Err(format!(
                            "本地模型推理未完成，请检查输入预览或重新识别：{error}"
                        ))
                        .into(),
                    };
                    result.preview = preview;
                    result
                });
            }
        }
    }

    pub(super) fn set_ink_math_mode(&mut self, mode: InkMathMode) {
        if self.ink_math_mode != mode {
            self.cancel_ink();
            self.ink_math_mode = mode;
            self.auto_gate.set_enabled(mode != InkMathMode::Off);
        }
    }

    pub(super) fn cancel_math(&mut self) {
        self.running_candidate = None;
        self.calculating_ink = None;
        self.math_ticket = None;
        self.math_input = None;
        self.math_worker.cancel();
    }

    pub(super) fn start_math(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce() -> MathResult + Send + 'static,
    ) {
        self.start_math_placed(ctx, None, work);
    }

    /// `placement` fits the answer to the writing it was computed from. The
    /// side (below equations, right of everything else) comes from the
    /// standard result kind inside the personalized output, so the handwritten
    /// rendering and the fallback-standard rendering land in the same spot.
    pub(super) fn start_math_placed(
        &mut self,
        ctx: &egui::Context,
        placement: Option<features::AnswerPlacement>,
        work: impl FnOnce() -> MathResult + Send + 'static,
    ) {
        if self.math_worker.busy() {
            self.math_result = "上一项计算仍在排空，请稍后重试".into();
            return;
        }
        let ticket = new_id();
        self.math_ticket = Some(ticket.clone());
        self.math_input = Some(self.current_math_input());
        let context = ContextToken::capture(&self.session.document);
        self.math_result = "后台计算中；修改输入、换页或取消将丢弃结果".into();
        // Freeze local style and font once; later learning cannot mutate this answer.
        // Profile validation bounds this clone to 65,536 sample points. The output
        // alphabet is only known inside work(), so filtering here would lose glyphs.
        let profile = self
            .handwriting
            .enabled
            .then(|| self.handwriting.profile.clone());
        let font = self.export_resources.handwriting_font();
        self.math_worker.start(ctx.clone(), move || {
            let result = match personalize_math(work(), profile.as_ref(), font.as_ref()) {
                Ok((text, object, fallback)) => {
                    let below = fallback
                        .as_ref()
                        .or(object.as_ref())
                        .is_some_and(|kind| matches!(kind, ObjectKind::Text { .. }));
                    let place = |kind: ObjectKind| match &placement {
                        Some(placement) => features::place_answer(
                            kind,
                            placement.source_bounds,
                            placement.width,
                            below,
                        ),
                        None => kind,
                    };
                    Ok((text, object.map(place), fallback.map(place)))
                }
                Err(error) => Err(error),
            };
            (ticket, context, result)
        });
    }

    pub(super) fn poll_ink(
        &mut self,
        ctx: &egui::Context,
        now: Instant,
        recognize: impl FnOnce(&[Vec<StrokePoint>]) -> std::result::Result<Recognition, String>
        + Send
        + 'static,
    ) {
        self.poll_ink_result(ctx, now, move |strokes| recognize(strokes).into());
    }

    pub(super) fn poll_ink_result(
        &mut self,
        ctx: &egui::Context,
        now: Instant,
        recognize: impl FnOnce(&[Vec<StrokePoint>]) -> HwrResult + Send + 'static,
    ) {
        let context = ContextToken::capture(&self.session.document);
        if !self.session.effective_visible()
            || self.ink_context.as_ref().is_some_and(|old| old != &context)
        {
            self.cancel_ink();
            return;
        }
        if self.ink_math_mode == InkMathMode::Off {
            // Off is the default steady state, not a user cancellation event.
            // Manual math is independent of recognition and must survive polling.
            self.cancel_recognition();
            return;
        }
        if !self.math_worker.busy()
            && !self.hwr_worker.busy()
            && !ctx.input(|i| i.pointer.any_down())
            && self.gesture.is_none()
            && let Some(request) = self.auto_gate.poll(now, &context)
        {
            self.save_candidate_edit();
            self.active_candidate = None;
            self.expression.clear();
            self.recognition = None;
            self.ink_ticket = Some(request.id().to_owned());
            let work = request.clone();
            self.neural_raw = None;
            self.ink_diagnostic =
                (self.hwr_backend == HwrBackend::TexTeller).then(|| InkDiagnostic {
                    request_id: request.id().to_owned(),
                    context: request.context.clone(),
                    stroke_points: request.strokes.iter().map(Vec::len).collect(),
                    excluded: self.ink_excluded,
                    texture: None,
                    error: None,
                });
            let (max_strokes, max_points) = match self.hwr_backend {
                HwrBackend::Template => (32, 8192),
                HwrBackend::TexTeller => (128, 32768),
            };
            self.hwr_worker.start(ctx.clone(), move || {
                let result = if work.strokes.len() > max_strokes
                    || work.strokes.iter().map(Vec::len).sum::<usize>() > max_points
                {
                    Err(format!(
                        "本次识别超过 GUI 预算（{max_strokes} 笔 / {max_points} 点），请分段书写"
                    ))
                    .into()
                } else {
                    recognize(&work.strokes)
                };
                (work, result)
            });
            self.math_result = "后台识别中；识别完成后点击笔迹右上角按钮才计算或绘图".into();
            self.status = self.math_result.clone();
            self.calculation = Some(request);
        }
    }

    pub(super) fn poll_workers(&mut self, ctx: &egui::Context) {
        self.sync_ink_cache();
        if !self.session.effective_visible()
            || self.compact_entry()
            || self.allow_close
            || self.confirm_close
            || self
                .ink_context
                .as_ref()
                .is_some_and(|context| *context != ContextToken::capture(&self.session.document))
            || self.calculation.as_ref().is_some_and(|request| {
                request.context != ContextToken::capture(&self.session.document)
            })
        {
            self.cancel_ink();
        }
        if !self.session.effective_visible()
            || self.compact_entry()
            || self.allow_close
            || self.confirm_close
        {
            self.plot_worker.cancel();
            self.plot_points = None;
        }
        poll_model_load(
            &mut self.model_loader,
            &mut self.neural_recognizer,
            &mut self.loaded_model_dir,
            &self.model_dir,
            self.hwr_backend,
            &mut self.model_status,
        );

        if self
            .math_input
            .as_ref()
            .is_some_and(|input| input != &self.current_math_input())
        {
            self.cancel_math();
        }
        if let Some(result) = self.math_worker.take() {
            match result {
                Ok((ticket, context, result))
                    if self.math_ticket.as_ref() == Some(&ticket)
                        && context == ContextToken::capture(&self.session.document)
                        && self.session.effective_visible() =>
                {
                    self.math_ticket = None;
                    match result {
                        Ok((mut text, object, fallback)) => {
                            let result_kind = object
                                .as_ref()
                                .map(|kind| {
                                    if matches!(kind, board_core::ObjectKind::Math { .. }) {
                                        "math"
                                    } else {
                                        "text"
                                    }
                                })
                                .unwrap_or("none");
                            self.logger
                                .debug("math worker finished", &[("result_kind", result_kind)]);
                            if let Some(kind) = object {
                                let candidate = self.running_candidate.take();
                                let insert = |app: &mut Self, kind| {
                                    if let Some((id, epoch)) = &candidate {
                                        if *epoch != app.ink_cache.epoch {
                                            return None;
                                        }
                                        app.add_candidate_result(id, kind)
                                    } else {
                                        let revision = app.session.document.revision;
                                        app.add(kind);
                                        Some(app.session.document.revision != revision)
                                    }
                                };
                                match insert(self, kind) {
                                    Some(true) => {}
                                    Some(false) => {
                                        let error = self.status.clone();
                                        // History failures are atomic; retry only the original
                                        // standard answer, using the same candidate identity.
                                        text = if let Some(standard) = fallback {
                                            match insert(self, standard) {
                                                Some(true) => format!(
                                                    "{text}\n个人笔迹写入失败，已改用标准字体：{error}"
                                                ),
                                                Some(false) => format!(
                                                    "个人笔迹写入失败：{error}\n标准答案写入失败：{}",
                                                    self.status
                                                ),
                                                None => format!(
                                                    "个人笔迹写入失败：{error}\n候选上下文已失效或结果已存在，未重试写入"
                                                ),
                                            }
                                        } else {
                                            error
                                        };
                                    }
                                    None => {
                                        text = "候选上下文已失效或结果已存在，未写入".into();
                                    }
                                }
                            }
                            self.status = text.clone();
                            self.math_result = text;
                        }
                        Err(error) => {
                            self.status = error.clone();
                            self.math_result = error;
                        }
                    }
                }
                Err(error) => self.math_result = error,
                _ => {}
            }
        }
        if !self.math_worker.busy() {
            self.running_candidate = None;
            self.calculating_ink = None;
            self.math_ticket = None;
            self.math_input = None;
        }
        if let Some(result) = self.hwr_worker.take() {
            let (outcome, candidates) = match result.as_ref() {
                Ok((_, hwr)) => match hwr.recognition.as_ref() {
                    Ok(recognition) => ("ok", recognition.candidates.len()),
                    Err(_) => ("failed", 0),
                },
                Err(_) => ("failed", 0),
            };
            self.logger.info(
                "recognition finished",
                &[
                    (
                        "backend",
                        if self.hwr_backend == HwrBackend::TexTeller {
                            "texeller"
                        } else {
                            "template"
                        },
                    ),
                    ("outcome", outcome),
                    ("candidates", &candidates.to_string()),
                ],
            );
            match result {
                Ok((request, result))
                    if self.session.effective_visible()
                        && self.ink_math_mode != InkMathMode::Off
                        && self.ink_ticket.as_deref() == Some(request.id())
                        && self.calculation.as_ref() == Some(&request)
                        && request.context == ContextToken::capture(&self.session.document) =>
                {
                    self.ink_ticket = None;
                    if let Some(diagnostic) = &mut self.ink_diagnostic
                        && diagnostic.request_id == request.id()
                        && diagnostic.context == request.context
                    {
                        diagnostic.error = result.recognition.as_ref().err().cloned();
                        diagnostic.texture = result.preview.map(|preview| {
                            ctx.load_texture(
                                format!("texteller-input-{}", request.id()),
                                egui::ColorImage::from_gray(preview.size, &preview.gray),
                                egui::TextureOptions::NEAREST,
                            )
                        });
                    }
                    self.neural_raw = result.neural;
                    match result.recognition {
                        Ok(recognition) => {
                            self.expression = recognition.text.clone();
                            self.math_result = if features::function_expression(&self.expression)
                                .is_some()
                            {
                                "函数候选：点击笔迹右上角曲线坐标图标生成函数图像，也可在数学/设置中纠错"
                                    .into()
                            } else {
                                "候选已识别：点击笔迹右上角计算器图标计算，也可在数学/设置中纠错"
                                    .into()
                            };
                            self.recognition = Some(recognition);
                        }
                        Err(error) => {
                            self.math_result = format!(
                                "识别失败，未写入；可在数学面板手动输入后计算或写板：{error}"
                            )
                        }
                    }
                    self.active_candidate = self.ink_cache.insert(
                        &self.session.document,
                        request,
                        self.ink_sources.clone(),
                        self.recognition.clone(),
                        self.neural_raw.clone(),
                        self.hwr_backend,
                    );
                    if self.active_candidate.is_none() {
                        self.calculation = None;
                        self.math_result =
                            "候选未缓存：缺少合法原笔迹或达到内存安全预算；已有候选仍保留".into();
                    }
                    self.status = self.math_result.clone();
                }
                Err(error) => self.math_result = error,
                _ => {}
            }
        }
        self.sync_plot_state();
        if let Some(result) = self.plot_worker.take() {
            self.logger.debug("plot worker finished", &[]);
            match result {
                Ok((context, object, report))
                    if context == ContextToken::capture(&self.session.document)
                        && self.selected_object().as_ref() == Some(&object) =>
                {
                    if let Some(state) = &mut self.plot_points {
                        state.report = Some(Ok(report));
                    }
                }
                Err(error) => {
                    if let Some(state) = &mut self.plot_points {
                        state.report = Some(Err(features::plot_text(&error)));
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn cancel_job(&mut self, id: &str) {
        self.cancel_authorization();
        if let Ok(request) = Request::new(
            format!("runtime:gui:{}", new_id()),
            "jobs.cancel",
            serde_json::json!({"job_id": id}),
        ) {
            let out = self.session.handle(request);
            self.messages(out);
        }
    }

    pub(super) fn save_candidate_edit(&mut self) {
        if let Some(entry) = self
            .active_candidate
            .as_deref()
            .and_then(|id| self.ink_cache.get_mut(id))
            && entry.expression != self.expression
            && self.expression.len() <= 4096
        {
            entry.expression = self.expression.clone();
        }
    }

    pub(super) fn sync_ink_cache(&mut self) {
        self.ink_cache.sync(&self.session.document);
        if self.active_candidate.as_deref().is_some_and(|id| {
            self.ink_cache.get(id).is_none_or(|entry| {
                entry.request.context.page_id != self.session.document.current_page().id
            })
        }) {
            self.cancel_ink();
        }
    }

    pub(super) fn activate_candidate(&mut self, id: &str) -> bool {
        self.sync_ink_cache();
        self.save_candidate_edit();
        let Some(entry) = self.ink_cache.get(id) else {
            return false;
        };
        if entry.request.context.page_id != self.session.document.current_page().id {
            return false;
        }
        self.expression = entry.expression.clone();
        self.recognition = entry.recognition.clone();
        self.neural_raw = entry.neural.clone();
        let mut request = entry.request.clone();
        request.context = ContextToken::capture(&self.session.document);
        self.ink_context = Some(request.context.clone());
        self.calculation = Some(request);
        self.active_candidate = Some(id.to_owned());
        true
    }

    // None means no insertion was attempted, not a transaction failure to retry.
    pub(super) fn add_candidate_result(&mut self, id: &str, kind: ObjectKind) -> Option<bool> {
        self.sync_ink_cache();
        let entry = self.ink_cache.get(id)?;
        if entry.result_present
            || entry.request.context.page_id != self.session.document.current_page().id
        {
            return None;
        }
        // This identity is already excluded from local validation before changed() runs.
        let result_id = entry.result_id.clone();
        let revision = self.session.document.revision;
        self.apply(vec![Operation::Add {
            object: BoardObject {
                id: result_id,
                kind,
            },
        }]);
        Some(self.session.document.revision != revision)
    }

    pub(super) fn ink_started(&mut self) {
        self.save_candidate_edit();
        self.active_candidate = None;
        self.auto_gate.input_started();
        self.ink_ticket = None;
        self.cancel_math();
        self.hwr_worker.cancel();
        self.recognition = None;
        self.neural_raw = None;
        self.ink_diagnostic = None;
        self.calculation = None;
    }

    pub(super) fn cancel_ink(&mut self) {
        self.cancel_math();
        self.cancel_recognition();
        self.multi_gestures.clear();
    }

    pub(super) fn cancel_recognition(&mut self) {
        self.save_candidate_edit();
        self.active_candidate = None;
        self.ink_sources.clear();
        self.ink_ticket = None;
        self.hwr_worker.cancel();
        self.auto_gate.cancel();
        self.ink.clear();
        self.ink_excluded = (0, 0);
        self.ink_diagnostic = None;
        self.ink_context = None;
        self.recognition = None;
        self.neural_raw = None;
        self.calculation = None;
    }
    pub(super) fn changed(&mut self) {
        self.cancel_ink();
        self.sync_ink_cache();
        self.plot_worker.cancel();
        self.plot_points = None;
        self.messages(vec![
            Event::new("document_changed", self.session.state()).into(),
            Event::new("state_changed", self.session.state()).into(),
        ]);
    }
    pub(super) fn apply(&mut self, operations: Vec<Operation>) {
        if operations.is_empty() {
            return;
        }
        let s = &mut self.session;
        let page = s.document.current_page().id.clone();
        let revision = s.document.revision;
        // #region debug-point D:history
        let debug_started = debug_ink::start();
        // #endregion
        let result = s
            .history
            .apply(&mut s.document, &page, revision, &operations);
        // #region debug-point D:history
        let debug_us = debug_ink::micros(debug_started);
        // #region debug-point B:history
        #[cfg(test)]
        debug_ink::record(7, "history_apply", debug_us);
        // #endregion
        if debug_started.is_some() {
            debug_ink::event(
                "B",
                "gui.rs:history.apply",
                serde_json::json!({
                    "history_us": debug_us, "operations": operations.len(), "ok": result.is_ok(),
                    "revision_before": revision, "revision_after": s.document.revision,
                    "page_after": debug_ink::counts(s.document.current_page()),
                    "can_undo": s.history.can_undo(), "can_redo": s.history.can_redo(),
                }),
            );
        }
        // #endregion
        match result {
            Ok(()) => self.changed(),
            Err(error) => self.status = error.to_string(),
        }
    }
    pub(super) fn commit_line(&mut self, object: BoardObject, adding: bool, endpoints: &[usize]) {
        let s = &mut self.session;
        match editing::commit_line(&mut s.document, &mut s.history, object, adding, endpoints) {
            Ok(()) => self.changed(),
            Err(e) => self.status = e.to_string(),
        }
    }
}
