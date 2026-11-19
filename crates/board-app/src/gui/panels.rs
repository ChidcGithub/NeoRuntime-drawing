// 文件/数学/模型面板与授权、导出流程。从 gui.rs 拆出（纯移动，行为不变）。
use super::*;

impl BoardApp {
    pub(super) fn panels(&mut self, ctx: &egui::Context) {
        self.sync_handwriting_context();
        if self.files {
            let mut open = true;
            egui::Window::new("文件：明确路径，不扫描个人文件")
                .default_width((ctx.content_rect().width() - 40.0).clamp(80.0, 480.0))
                .max_size((ctx.content_rect().size() - Vec2::splat(40.0)).max(Vec2::splat(80.0)))
                .scroll([true, true])
                .constrain_to(ctx.content_rect())
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("板书路径")
                        .on_hover_text("兼容旧版 JSON；图片随板书资源包保存。");
                    ui.text_edit_singleline(&mut self.path);
                    ui.horizontal(|ui| {
                        if ActionButton::primary(egui::Id::new("files.save"), "保存")
                            .show(ui)
                            .clicked()
                        {
                            self.save();
                        }
                        if ActionButton::secondary(egui::Id::new("files.load"), "载入")
                            .show(ui)
                            .clicked()
                        {
                            if self.session.history.is_dirty(&self.session.document)
                                && !self.discard_load
                            {
                                self.status = "当前有未保存内容，请先保存或勾选确认丢弃".into();
                            } else {
                                match self
                                    .session
                                    .open_document(self.path.trim(), self.discard_load)
                                {
                                    Ok(messages) => {
                                        self.logger.info("document opened", &[]);
                                        self.handwriting.invalidate_context();
                                        self.ink_cache.clear();
                                        self.messages(messages);
                                        self.selected = None;
                                        self.gesture = None;
                                        self.discard_load = false;
                                        self.textures.clear();
                                        self.renderer.clear();
                                        self.thumbnails.clear();
                                        self.captured_asset = None;
                                        self.split_drag = None;
                                        self.agent_image = None;
                                        self.changed();
                                        self.status = "已载入板书及图片资源".into();
                                    }
                                    Err(e) => {
                                        self.logger.warn("document open failed", &[]);
                                        self.status = e.message;
                                    }
                                }
                            }
                        }
                        ui.checkbox(&mut self.discard_load, "确认载入时丢弃未保存内容");
                    });
                    ui.separator();
                    ui.label("PNG 图片路径")
                        .on_hover_text("只读取指定路径，不扫描文件；图片随板书保存。");
                    ui.text_edit_singleline(&mut self.image_path);
                    if ActionButton::secondary(egui::Id::new("files.import.image"), "导入图片")
                        .show(ui)
                        .clicked()
                    {
                        use std::io::Read;
                        let imported = (|| -> std::result::Result<String, String> {
                            let mut bytes = Vec::new();
                            std::fs::File::open(self.image_path.trim())
                                .map_err(|e| e.to_string())?
                                .take(16 * 1024 * 1024 + 1)
                                .read_to_end(&mut bytes)
                                .map_err(|e| e.to_string())?;
                            self.session
                                .resources
                                .import_png(bytes)
                                .map_err(|e| e.message)
                        })();
                        match imported {
                            Ok(asset_ref) => {
                                let resource = self.session.resources.get(&asset_ref).unwrap();
                                let factor = (600.0 / resource.width as f32)
                                    .min(400.0 / resource.height as f32)
                                    .min(1.0);
                                self.add(ObjectKind::Image {
                                    position: Point { x: 80.0, y: 80.0 },
                                    width: resource.width as f32 * factor,
                                    height: resource.height as f32 * factor,
                                    asset_ref,
                                });
                            }
                            Err(e) => self.status = e,
                        }
                    }
                    if self.captured_asset.is_some()
                        && ActionButton::secondary(
                            egui::Id::new("files.insert.capture"),
                            "将已返回截图插入当前页（不上传）",
                        )
                        .show(ui)
                        .clicked()
                        && let Some(asset_ref) = self.captured_asset.take()
                        && let Some(resource) = self.session.resources.get(&asset_ref)
                    {
                        let factor = (600.0 / resource.width as f32)
                            .min(400.0 / resource.height as f32)
                            .min(1.0);
                        self.add(ObjectKind::Image {
                            position: Point { x: 80.0, y: 80.0 },
                            width: resource.width as f32 * factor,
                            height: resource.height as f32 * factor,
                            asset_ref,
                        });
                    }
                    ui.label("导出路径（当前页）").on_hover_text("不含工具条。");
                    ui.text_edit_singleline(&mut self.export_path);
                    ui.horizontal(|ui| {
                        if ActionButton::secondary(egui::Id::new("files.export.svg"), "导出 SVG")
                            .show(ui)
                            .clicked()
                        {
                            self.export(false);
                        }
                        if ActionButton::secondary(egui::Id::new("files.export.png"), "导出 PNG")
                            .show(ui)
                            .clicked()
                        {
                            self.export(true);
                        }
                    });
                    ui.label(&self.status);
                });
            self.files = open;
        }
        if self.math && self.mode == AppMode::Blackboard {
            let mut open = true;
            egui::Window::new("数学输入与识别候选")
                .default_width((ctx.content_rect().width() - 40.0).clamp(80.0, 480.0))
                .max_size((ctx.content_rect().size() - Vec2::splat(40.0)).max(Vec2::splat(80.0)))
                .scroll([true, true])
                .constrain_to(ctx.content_rect())
                .open(&mut open)
                .show(ctx, |ui| {
                    let mut mode = self.ink_math_mode;
                    ui.label("手写识别（默认关闭）")
                        .on_hover_text("启用后停笔 2.5 秒触发识别；仅本次运行有效，不自动计算或绘图。");
                    ui.radio_value(&mut mode, InkMathMode::Off, "关闭手写计算/识别")
                        .on_hover_text("关闭识别仍保留已加载模型；释放内存请点击“卸载模型”或切换模板后端。");
                    ui.radio_value(&mut mode, InkMathMode::Confirm, "启用手写识别（点击图标才计算/绘图）");
                    self.set_ink_math_mode(mode);
                    let mut backend = self.hwr_backend;
                    ui.horizontal(|ui| {
                        ui.label("识别后端");
                        ui.radio_value(&mut backend, HwrBackend::Template, "模板（默认）");
                        ui.radio_value(&mut backend, HwrBackend::TexTeller, "TexTeller 本地模型");
                    });
                    self.set_hwr_backend(backend);
                    if backend == HwrBackend::TexTeller {
                        ui.label("模型目录（绝对路径）")
                            .on_hover_text("手动加载，不扫描或下载；每次最多 128 笔 / 32768 点。");
                        ui.add_enabled_ui(!self.model_loader.busy(), |ui| {
                            if ui.text_edit_singleline(&mut self.model_dir).changed() {
                                self.cancel_ink();
                                self.model_status = "目录已修改；请点击后台加载 / 重载模型".into();
                            }
                        });
                        ui.horizontal(|ui| {
                            if ActionButton::primary(
                                egui::Id::new("model.load"),
                                "后台加载 / 重载模型",
                            )
                            .enabled(self.model_load_ready())
                            .show(ui)
                            .clicked()
                            {
                                self.load_model(ctx);
                            }
                            if ActionButton::danger(
                                egui::Id::new("model.unload"),
                                "卸载模型",
                            )
                            .enabled(
                                self.neural_recognizer.is_some()
                                    || self.model_loader.busy()
                                    || !self.loaded_model_dir.is_empty(),
                            )
                            .show(ui)
                            .clicked()
                            {
                                self.unload_model();
                            }
                        });
                        ui.label(&self.model_status);
                        if self.model_loader.busy() || self.hwr_worker.busy() {
                            ui.small("后台任务未排空；取消/卸载不强杀线程，排空前不能重载。");
                        }
                        ui.small("关闭识别仍保留模型；卸载或切模板会释放，进行中的任务排空后完成释放。");
                        ui.small("默认优先建议文件齐全的 models/texteller-int8；仅检查存在，不保证内容或哈希有效，仍须点击加载后校验。");
                        ui.colored_label(Color32::YELLOW, "低内存设备建议手动加载独立量化目录（models/texteller-int8）。FP32 模型内存占用高，不保证 3GB 内运行；目录建议不代表已量化或整机内存达标。");
                        ui.colored_label(Color32::YELLOW, "模型可能看错，请核对后点击计算/绘图。");
                    }
                    if let Some(diagnostic) = &self.ink_diagnostic
                        && diagnostic.context == ContextToken::capture(&self.session.document)
                        && self.calculation.as_ref().is_some_and(|request| request.id() == diagnostic.request_id)
                    {
                        ui.separator();
                        ui.collapsing("本次识别输入", |ui| {
                            ui.small(format!("请求：{}", diagnostic.request_id));
                            ui.label(format!("快照：{} 笔 / {} 点；逐笔点数（书写顺序）：{:?}", diagnostic.stroke_points.len(), diagnostic.stroke_points.iter().sum::<usize>(), diagnostic.stroke_points));
                            ui.label(format!("未纳入的非相邻笔迹：{} 笔 / {} 点（仍保留在板书中）", diagnostic.excluded.0, diagnostic.excluded.1));
                            if let Some(texture) = &diagnostic.texture {
                                let side = ui.available_width().clamp(1.0, 448.0);
                                ui.image((texture.id(), Vec2::splat(side)));
                                ui.small("448×448 模型输入预览")
                                    .on_hover_text("实际预处理张量逆归一化：灰度 = (值 × 0.15394445 + 0.9545467) × 255；右/下补零约为 243 灰度。推理失败时仍保留已准备的输入。");
                            } else {
                                ui.label(if self.hwr_worker.busy() { "等待本次输入预览…" } else { "本次未生成预览（未进入预处理或预处理失败）" });
                            }
                            ui.small("仅本地笔迹预览，不保存或上传。")
                                .on_hover_text("不是桌面截图；继续书写、切换上下文/后端或取消后清除。");
                        });
                        if let Some(error) = &diagnostic.error {
                            ui.colored_label(Color32::YELLOW, error);
                        }
                    }
                    if let Some(raw) = &self.neural_raw {
                        ui.collapsing("查看原始 LaTeX / 模型分数（非准确概率）", |ui| {
                            ui.label(&raw.latex);
                            ui.label(format!("finished={}，tokens={}，mean logprob={:.6}", raw.finished, raw.generated_tokens, raw.mean_log_probability));
                        });
                    }
                    ui.label("识别结果须核对/纠错后点击计算；分数不是正确概率。")
                        .on_hover_text("离线模板仅适合分离直立符号及简单上下标/分数，不支持任意手写或所有高数。");
                    if let Some(recognition) = &self.recognition {
                        for candidate in &recognition.candidates {
                            let backend = self.active_candidate.as_deref().and_then(|id| self.ink_cache.get(id)).map_or(self.hwr_backend, |entry| entry.backend);
                            let label = if backend == HwrBackend::TexTeller {
                                format!("{}  （模型候选，未校准）", candidate.text)
                            } else {
                                format!("{}  相似度 {:.0}%", candidate.text, candidate.confidence * 100.0)
                            };
                            if ActionButton::quiet(
                                egui::Id::new(("hwr.candidate", &candidate.text, candidate.confidence.to_bits())),
                                label,
                            )
                            .selected(self.expression == candidate.text)
                            .show(ui)
                            .clicked()
                            {
                                self.expression = candidate.text.clone();
                                self.math_ticket = None;
                                self.math_input = None;
                                self.math_worker.cancel();
                                self.hwr_worker.cancel();
                            }
                        }
                    }
                    if self.calculation.is_some()
                        && ActionButton::primary(
                            egui::Id::new("hwr.confirm"),
                            "确认纠错并计算写回 / 生成函数图像",
                        )
                        .enabled(!self.ink_action_busy() && !self.expression.trim().is_empty())
                        .show(ui)
                        .clicked()
                    {
                        self.confirm_calculation(ctx);
                    }
                    if ActionButton::secondary(egui::Id::new("hwr.cancel.candidates"), "取消候选 / 开始新表达式")
                        .show(ui)
                        .clicked()
                    {
                        self.cancel_ink();
                    }
                    ui.collapsing("学习个人符号模板（只在本机）", |ui| {
                        ui.label("启用识别，单独写一个 1–4 笔符号；停笔后填写正确符号。")
                            .on_hover_text("只学习个人符号模板，不训练通用手写模型。");
                        ui.text_edit_singleline(&mut self.template_label);
                        if ActionButton::secondary(
                            egui::Id::new("hwr.template.learn"),
                            "学习当前候选笔迹",
                        )
                        .show(ui)
                        .clicked()
                        {
                            self.math_result = match self.calculation.as_ref() {
                                Some(request) if request.context == ContextToken::capture(&self.session.document) => {
                                    match self.recognizer.register_template(self.template_label.trim(), &request.strokes) {
                                        Ok(()) => "已加入本次识别器；请明确选择路径保存以供下次加载".into(),
                                        Err(e) => e.to_string(),
                                    }
                                }
                                _ => "请先单独书写符号并等待候选；当前候选为空或已过期".into(),
                            };
                        }
                        ui.label("模板 JSON 路径（保存会覆盖该文件）")
                            .on_hover_text("仅使用指定路径，不自动扫描或加载。");
                        ui.text_edit_singleline(&mut self.template_path);
                        ui.horizontal(|ui| {
                            if ActionButton::secondary(
                                egui::Id::new("hwr.template.save"),
                                "保存模板到指定路径",
                            )
                            .enabled(!self.template_path.trim().is_empty())
                            .show(ui)
                            .clicked()
                            {
                                self.math_result = self.recognizer.save(self.template_path.trim())
                                    .map(|()| format!("模板已保存：{}", self.template_path.trim()))
                                    .unwrap_or_else(|e| e.to_string());
                            }
                            if ActionButton::secondary(
                                egui::Id::new("hwr.template.load"),
                                "加载指定模板",
                            )
                            .enabled(!self.template_path.trim().is_empty())
                            .show(ui)
                            .clicked()
                            {
                                match InkRecognizer::load(self.template_path.trim()) {
                                    Ok(recognizer) => { self.cancel_ink(); self.expression.clear(); self.recognizer = recognizer; self.math_result = "已加载模板，将用于后续识别".into(); }
                                    Err(e) => self.math_result = e.to_string(),
                                }
                            }
                        });
                    });
                    self.sync_handwriting_context();
                    let handwriting_font = self.export_resources.handwriting_font();
                    if self.handwriting.ui(ui, handwriting_font.as_ref()) {
                        self.cancel_math();
                    }
                    if let Some(ObjectKind::Handwritten { text, .. }) = self.selected.as_ref().and_then(|id| {
                        self.session.document.current_page().objects.iter().find(|object| &object.id == id).map(|object| &object.kind)
                    }) {
                        ui.label("所选手写答案的标准文字：");
                        ui.label(text);
                        if ActionButton::secondary(egui::Id::new("hwr.answer.copy"), "复制所选答案标准文字")
                            .show(ui)
                            .clicked()
                        {
                            ui.ctx().copy_text(text.clone());
                        }
                    }
                    if ui.text_edit_singleline(&mut self.expression).changed() { self.cancel_math(); self.hwr_worker.cancel(); }
                    if self.math_worker.busy()
                        && ActionButton::danger(
                            egui::Id::new("hwr.cancel.calc"),
                            "取消后台计算（不写回）",
                        )
                        .show(ui)
                        .clicked()
                    {
                        self.cancel_math();
                        self.math_result = "已取消；旧工作排空前不启动新任务".into();
                    }
                    ui.label("方程组经结式消元在范围内完备求解，重合曲线单独提示。")
                        .on_hover_text("两式用英文分号分隔；每式仅支持总次数 ≤2 的 x/y 多项式。未找到候选不等于无解。");
                    ui.horizontal(|ui| {
                        ui.label("x 范围 / 积分上下限");
                        let mut changed = ui.add(egui::DragValue::new(&mut self.math_bounds.x_min)).changed();
                        changed |= ui.add(egui::DragValue::new(&mut self.math_bounds.x_max)).changed();
                        ui.label("y 范围");
                        changed |= ui.add(egui::DragValue::new(&mut self.math_bounds.y_min)).changed();
                        changed |= ui.add(egui::DragValue::new(&mut self.math_bounds.y_max)).changed();
                        if changed { self.cancel_math(); }
                    });
                    ui.horizontal(|ui| {
                        ui.label("数值微分点 x");
                        if ui.add(egui::DragValue::new(&mut self.derivative_at)).changed() { self.cancel_math(); }
                        if ActionButton::secondary(egui::Id::new("math.derivative"), "数值微分")
                            .show(ui)
                            .clicked()
                        {
                            let expression = self.expression.clone();
                            let at = self.derivative_at;
                            self.start_math(ctx, move || board_math::derivative(&expression, at)
                                .map(|v| (format!("局部导数近似 ≈ {v}"), None)).map_err(|e| e.to_string()));
                        }
                        if ActionButton::secondary(egui::Id::new("math.integrate"), "数值定积分")
                            .show(ui)
                            .clicked()
                        {
                            let expression = self.expression.clone();
                            let bounds = self.math_bounds;
                            self.start_math(ctx, move || board_math::integrate(&expression, bounds.x_min, bounds.x_max, Default::default())
                                .map(|v| (format!("指定有限区间积分近似 ≈ {v}"), None)).map_err(|e| e.to_string()));
                        }
                    });
                    ui.label("数值微分/积分仅为近似，不支持广义积分。")
                        .on_hover_text("请输入 f(x) 表达式，而非等式。");
                    ui.collapsing("数学语法与限制", |ui| {
                        ui.label("输入后点击计算：simplify(x+1=2)、diff(x^3,x)、integrate(3*x^2,x)。");
                        ui.label("符号引擎支持初等函数求导、标准形积分与方程求解；积分常数取 0。");
                        ui.small("数学任务单槽无排队；默认数值预算512网格/30000次求值/500万词元计算。取消只丢弃结果，旧工作排空前不再启动；不承诺硬实时截止。");
                    });
                    ui.horizontal(|ui| {
                        for label in ["计算", "化简", "解方程"] {
                            if ActionButton::primary(egui::Id::new(("math.run", label)), label)
                                .show(ui)
                                .clicked()
                            {
                                let expression = self.expression.clone();
                                let bounds = self.math_bounds;
                                self.start_math(ctx, move || {
                                    let result = if label == "化简" { board_math::simplify(&expression) } else { features::calculation_output(&expression, bounds, true).map(|(text, _)| text) };
                                    result.map(|text| (text, None)).map_err(|e| e.to_string())
                                });
                            }
                        }
                        if ActionButton::secondary(egui::Id::new("math.plot"), "绘制函数")
                            .enabled(self.calculation.is_none())
                            .show(ui)
                            .clicked()
                        {
                            match board_math::classify_plot(&self.expression) {
                                Ok(board_math::PlotKind::Explicit(expression) | board_math::PlotKind::Implicit(expression)) => self.add(ObjectKind::FunctionPlot {
                                    position: Point { x: 100.0, y: 80.0 },
                                    width: 500.0,
                                    height: 350.0,
                                    expressions: vec![expression],
                                    x_min: -10.0,
                                    x_max: 10.0,
                                    y_min: -7.0,
                                    y_max: 7.0,
                                }),
                                Err(e) => self.math_result = e.to_string(),
                            }
                        }
                    });
                    ui.label(&self.math_result);
                    if ActionButton::primary(
                        egui::Id::new("math.write"),
                        "将手动输入计算后写到板上",
                    )
                    .enabled(self.calculation.is_none())
                    .show(ui)
                    .clicked()
                        && !self.expression.trim().is_empty()
                    {
                        let expression = self.expression.clone();
                        let bounds = self.math_bounds;
                        let color = self.style.color;
                        self.start_math(ctx, move || {
                            let output = features::calculation_display(&expression, bounds, true).map_err(|e| e.to_string())?;
                            let result = output.text.clone();
                            let kind =
                                output.into_kind(Point { x: 80.0, y: 80.0 }, color, 26.0);
                            Ok((result, Some(kind)))
                        });
                    }
                    if ActionButton::secondary(egui::Id::new("math.reset.plot"), "复原所选图像/坐标")
                        .show(ui)
                        .on_hover_text("选择函数图后可滚轮缩放、拖动平移；复原恢复默认范围。")
                        .clicked()
                        && let Some(mut object) = self.selected_object()
                        && matches!(object.kind, ObjectKind::FunctionPlot { .. } | ObjectKind::CoordinateSystem { .. })
                    {
                        match &mut object.kind {
                            ObjectKind::FunctionPlot { x_min, x_max, y_min, y_max, .. } => {
                                *x_min = -10.0; *x_max = 10.0; *y_min = -7.0; *y_max = 7.0;
                            }
                            ObjectKind::CoordinateSystem { scale, .. } => *scale = 40.0,
                            _ => {}
                        }
                        self.apply(vec![Operation::Update { object }]);
                    }
                });
            self.math = open;
            self.sync_handwriting_context();
        }
        if self.confirm_close {
            egui::Window::new("有未保存的板书")
                .default_width((ctx.content_rect().width() - 40.0).clamp(80.0, 400.0))
                .max_size((ctx.content_rect().size() - Vec2::splat(40.0)).max(Vec2::splat(80.0)))
                .scroll([true, true])
                .constrain_to(ctx.content_rect())
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("退出将丢失未保存内容。请保存或明确选择丢弃。");
                    ui.text_edit_singleline(&mut self.path);
                    ui.horizontal(|ui| {
                        if ActionButton::primary(egui::Id::new("exit.save"), "保存并退出")
                            .show(ui)
                            .clicked()
                            && self.save()
                        {
                            self.allow_close = true;
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                        if ActionButton::danger(egui::Id::new("exit.discard"), "丢弃并退出")
                            .show(ui)
                            .clicked()
                        {
                            self.allow_close = true;
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                        if ActionButton::secondary(egui::Id::new("exit.cancel"), "取消")
                            .show(ui)
                            .clicked()
                        {
                            self.confirm_close = false;
                        }
                    });
                    ui.label(&self.status);
                });
        }
    }
    pub(super) fn confirm_calculation(&mut self, ctx: &egui::Context) {
        if self.expression.len() > 4096 {
            self.math_result = "表达式超过 4096 字节，请缩短后重试".into();
            return;
        }
        if self.ink_action_busy()
            || self.expression.trim().is_empty()
            || self.ink_math_mode == InkMathMode::Off
            || !self.session.effective_visible()
            || self.gesture.is_some()
        {
            return;
        }
        self.sync_ink_cache();
        self.save_candidate_edit();
        let Some(id) = self.active_candidate.clone() else {
            return;
        };
        let Some(entry) = self.ink_cache.get(&id) else {
            return;
        };
        if entry.result_present {
            return;
        }
        let source_width = features::source_stroke_width(&entry.source);
        if !self.activate_candidate(&id) {
            return;
        }
        if features::function_expression(&self.expression).is_some() {
            self.plot_ink_function();
            return;
        }
        let request = self.calculation.as_ref().unwrap().clone();
        let size_hint = features::answer_size_hint(&request.strokes);
        self.calculate_ink(ctx, &request, size_hint, source_width);
        self.running_candidate = Some((id, self.ink_cache.epoch));
        self.calculating_ink = Some(features::ink_bounds(&request.strokes));
    }

    pub(super) fn calculate_ink(
        &mut self,
        ctx: &egui::Context,
        request: &CalculationRequest,
        size_hint: f32,
        width: Option<f32>,
    ) {
        let bounds = features::ink_bounds(&request.strokes);
        let expression = self.expression.clone();
        // Revalidate raw metadata against the current input, including manual corrections.
        let answer_prompt = self.neural_raw.as_ref().is_some_and(|raw| {
            board_hwr::latex_to_calculation(&raw.latex)
                .is_ok_and(|parsed| parsed.answer_prompt && parsed.expression == expression)
        });
        let math_bounds = self.math_bounds;
        let color = self.style.color;
        self.start_math_placed(
            ctx,
            Some(features::AnswerPlacement {
                source_bounds: bounds,
                width,
            }),
            move || {
                let output = features::calculation_display(&expression, math_bounds, answer_prompt)
                    .map_err(|e| e.to_string())?;
                let position = features::result_position(bounds, output.equation);
                let result = output.text.clone();
                Ok((result, Some(output.into_kind(position, color, size_hint))))
            },
        );
    }
    pub(super) fn authorized_asset_refs(&self) -> Vec<String> {
        if !self.authorize_assets {
            return Vec::new();
        }
        if let Some((_, _, asset)) = &self.agent_image {
            return vec![asset.clone()];
        }
        let mut assets = Vec::new();
        for object in &self.session.document.current_page().objects {
            if let ObjectKind::Image { asset_ref, .. } = &object.kind
                && !assets.contains(asset_ref)
            {
                assets.push(asset_ref.clone());
            }
        }
        assets
    }

    pub(super) fn cancel_authorization(&mut self) {
        self.authorization = None;
        self.agent_image = None;
        self.authorize_assets = false;
        self.authorize_write_back = false;
    }

    pub(super) fn take_host_request(
        &mut self,
        capture: bool,
    ) -> std::result::Result<Request, String> {
        if !capture && let Some((context, id, asset)) = &self.agent_image
            && (context != &ContextToken::capture(&self.session.document)
                || !self.session.document.current_page().objects.iter().any(|o| &o.id == id && matches!(&o.kind, ObjectKind::Image { asset_ref, .. } if asset_ref == asset)))
        {
            self.cancel_authorization();
            return Err("图片或页面已变化，请从图片按钮重新授权".into());
        }
        let mut params = serde_json::json!({
            "document_id": self.session.document.id,
            "page_id": self.session.document.current_page().id,
            "expected_revision": self.session.document.revision,
            "user_authorized": true,
        });
        if !capture {
            params["prompt"] = serde_json::json!(self.agent_prompt.trim());
            params["write_back"] = serde_json::json!(self.authorize_write_back);
            params["asset_refs"] = serde_json::json!(self.authorized_asset_refs());
        }
        self.cancel_authorization();
        let request = Request::new(
            format!("runtime:gui:{}", new_id()),
            if capture {
                "capture.request"
            } else {
                "agent.request"
            },
            params,
        )
        .map_err(|e| e.to_string())?;
        if capture {
            self.capture_requests.insert(
                request.id.clone(),
                CaptureContext {
                    context: ContextToken::capture(&self.session.document),
                    canvas_size: self.canvas_size,
                    stale: false,
                },
            );
        }
        Ok(request)
    }

    pub(super) fn request_host(&mut self, capture: bool) {
        if !self.hosted {
            self.cancel_authorization();
            self.status = "独立模式没有宿主；请求未发送".into();
            return;
        }
        match self.take_host_request(capture) {
            Ok(request) => {
                let out = self.session.handle(request);
                self.messages(out);
            }
            Err(e) => self.status = e,
        }
    }
    pub(super) fn authorization_panel(&mut self, ctx: &egui::Context) {
        let Some(capture) = self.authorization else {
            return;
        };
        let mut open = true;
        let response = egui::Window::new(if capture {
            "本次截图授权"
        } else {
            "本次 Agent 授权"
        })
        .default_width((ctx.content_rect().width() - 40.0).clamp(80.0, 400.0))
        .max_size((ctx.content_rect().size() - Vec2::splat(40.0)).max(Vec2::splat(80.0)))
        .scroll([true, true])
        .constrain_to(ctx.content_rect())
        .open(&mut open)
        .show(ctx, |ui| {
            let state = self.session.state();
            let safe = state["permissions"]["classroom_safe"] == true;
            let permitted = if capture {
                !safe && state["permissions"]["desktop_capture_allowed"] == true
            } else {
                state["permissions"]["agent_allowed"] == true
            };
            if capture {
                ui.label("宿主确认所有窗口隐藏后框选；成功截图自动插入原页面（可撤销），不会发送给 Agent。页面或版本变化时不自动插入。");
                if safe {
                    ui.colored_label(Color32::YELLOW, "课堂安全模式：拒绝截图，不会采集桌面。");
                }
            } else {
                ui.label("Agent 仅 hosted 宿主服务可用；独立模式不会发送。只发送本次明确输入的提示；图片与板书写入分别授权。默认只显示回答。");
                ui.text_edit_multiline(&mut self.agent_prompt);
                ui.checkbox(&mut self.authorize_write_back, "允许本次回答写入板书");
                ui.checkbox(
                    &mut self.authorize_assets,
                    if self.agent_image.is_some() {
                        "授权仅发送这张图片（不包含其他图片）"
                    } else {
                        "授权发送当前页图片资源（单独授权）"
                    },
                );
            }
            if !permitted {
                ui.label("宿主权限未允许，本次请求将拒绝。");
            }
            let authorize_enabled = permitted
                && self.hosted
                && (capture || !self.agent_prompt.trim().is_empty())
                && (self.agent_image.is_none() || self.authorize_assets || capture);
            if ActionButton::primary(
                egui::Id::new("agent.authorize"),
                "明确授权本次请求",
            )
            .enabled(authorize_enabled)
            .show(ui)
            .clicked()
            {
                self.request_host(capture);
            }
            ui.label(&self.status);
        });
        if let Some(response) = response {
            self.controls.push(response.response.rect);
        }
        if !open {
            self.cancel_authorization();
        }
    }
    pub(super) fn export(&mut self, png: bool) {
        if self.export_path.trim().is_empty() {
            self.status = "请输入导出文件路径".into();
            return;
        }
        let page = self.session.document.current_page();
        let w = self.canvas_size.x.ceil().clamp(1.0, 8192.0) as u32;
        let h = self.canvas_size.y.ceil().clamp(1.0, 8192.0) as u32;
        let blackboard = self.mode == AppMode::Blackboard;
        for object in &page.objects {
            if let ObjectKind::Image { asset_ref, .. } = &object.kind {
                if let Some(resource) = self.session.resources.get(asset_ref) {
                    if let Err(e) = self.export_resources.insert_png(asset_ref, &resource.bytes) {
                        self.status = e.to_string();
                        return;
                    }
                } else {
                    self.export_resources.remove_image(asset_ref);
                }
            }
        }
        let bytes = if png {
            board_render::export_png_with_resources(page, w, h, blackboard, &self.export_resources)
                .map_err(|e| e.to_string())
        } else {
            board_render::export_svg_with_resources(page, w, h, blackboard, &self.export_resources)
                .map(String::into_bytes)
                .map_err(|e| e.to_string())
        };
        self.status = match bytes.and_then(|data| {
            std::fs::write(self.export_path.trim(), data).map_err(|e| e.to_string())
        }) {
            Ok(()) => "已导出当前页".into(),
            Err(e) => e,
        };
    }
}
