// eframe::App 实现：窗口逻辑帧、UI 帧、worker 收尾与输入分发。从 gui.rs 拆出（纯移动）。
use super::*;

impl eframe::App for BoardApp {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // 窗口首帧初始化：黑板默认全屏（覆盖任务栏）；弹出时置顶一下即回正常层。
        match self.window_started {
            None => {
                self.window_started = Some(false);
                if self.mode == AppMode::Blackboard && !self.compact_entry() {
                    ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
                    ctx.send_viewport_cmd(ViewportCommand::WindowLevel(
                        egui::WindowLevel::AlwaysOnTop,
                    ));
                }
            }
            Some(false) => {
                self.window_started = Some(true);
                if self.mode == AppMode::Blackboard {
                    ctx.send_viewport_cmd(ViewportCommand::WindowLevel(egui::WindowLevel::Normal));
                }
            }
            Some(true) => {}
        }
        // 收起=最小化：用户点回任务栏恢复窗口 → 展开并退出效率模式。
        if self.collapsed
            && self.mode == AppMode::Blackboard
            && ctx.input(|i| i.viewport().minimized) == Some(false)
        {
            self.set_collapsed(ctx, false);
        }
        // #region debug-point D:logic
        let debug_logic_start = debug_ink::start();
        let mut debug_incoming = 0_u32;
        // #endregion
        for _ in 0..64 {
            let message = self.incoming.as_ref().map(|rx| rx.try_recv());
            let message = match message {
                Some(Ok(message)) => message,
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    self.disconnected();
                    break;
                }
                _ => break,
            };
            // #region debug-point D:logic
            debug_incoming += u32::from(debug_logic_start.is_some());
            // #endregion
            // 动态取色属 UI 态：theme.seed 在帧上下文就地处理，不进文档会话。
            if let Message::Request(request) = &message
                && request.method == "theme.seed"
            {
                let request = request.clone();
                let out = self.handle_theme_seed(ctx, request);
                self.messages(out);
            } else {
                self.incoming_message(message);
            }
        }
        // #region debug-point D:logic
        let debug_incoming_us = debug_ink::micros(debug_logic_start);
        // #endregion
        if let Some(request) = self.session.pending_window_request().cloned()
            && let Some(window) = frame.winit_window()
        {
            let applied =
                self.applied_window_request.as_deref() == Some(request.request_id.as_str());
            let confirmed = applied
                && window.is_visible() == Some(request.visible)
                && self.session.state()["owned_window_count"] == 1;
            window.set_visible(request.visible);
            self.applied_window_request = Some(request.request_id.clone());
            if confirmed {
                let out = self
                    .session
                    .acknowledge_window(&request.request_id, request.visible);
                self.messages(out);
            }
        }
        self.poll_local_capture();
        if self.allow_close
            || (self.session.closed && self.session.pending_window_request().is_none())
        {
            self.cancel_local_capture();
            self.allow_close = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        self.enforce_tool_mode();
        if let Some(window) = frame.winit_window() {
            let pass = self.mode == AppMode::Drawing
                && self.tool == Tool::Mouse
                && !self.compact_entry()
                && self.session.effective_visible()
                && !self.controls.is_empty()
                && !self.files
                && !self.math
                && !self.confirm_close
                && self.authorization.is_none()
                && !egui::Popup::is_any_open(ctx)
                && !ctx.input(|i| i.pointer.any_down())
                && cursor_position()
                    .and_then(|(x, y)| {
                        window.inner_position().ok().map(|p| {
                            let scale = ctx.pixels_per_point();
                            let local = features::cursor_logical((x, y), (p.x, p.y), scale);
                            features::passthrough_at(local, ctx.content_rect(), &self.controls)
                        })
                    })
                    .unwrap_or(false);
            if pass != self.passthrough {
                if window.set_cursor_hittest(!pass).is_ok() {
                    self.passthrough = pass;
                } else {
                    self.tool = Tool::Pen;
                    self.status = "系统不支持鼠标穿透，已恢复画笔".into();
                }
            }
        }
        // #region debug-point D:logic
        let debug_workers_start = debug_ink::start();
        // #endregion
        self.prepare_workers(ctx, Instant::now());
        // #region debug-point D:logic
        let debug_workers_us = debug_ink::micros(debug_workers_start);
        let debug_ink_start = debug_ink::start();
        // #endregion
        // GUI 发起的任务 60 秒后请求取消；隐藏租约仍只能由宿主停止确认释放。
        for id in self
            .jobs
            .iter()
            .filter(|(_, started)| started.elapsed() >= Duration::from_secs(60))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>()
        {
            self.cancel_job(&id);
        }
        ctx.request_repaint_after(Duration::from_millis(30));
        // #region debug-point D:logic
        let debug_ink_us = debug_ink::micros(debug_ink_start);
        let debug_logic_us = debug_ink::micros(debug_logic_start);
        debug_ink::sample(
            1,
            "D",
            "gui.rs:logic",
            ["logic", "incoming", "poll_workers", "poll_ink_jobs"],
            [
                debug_logic_us,
                debug_incoming_us,
                debug_workers_us,
                debug_ink_us,
            ],
            || {
                serde_json::json!({
                    "incoming_messages": debug_incoming, "jobs": self.jobs.len(),
                    "hwr_busy": self.hwr_worker.busy(), "math_busy": self.math_worker.busy(),
                    "plot_busy": self.plot_worker.busy(), "gesture_active": self.gesture.is_some(),
                    "visible": self.session.effective_visible(), "revision": self.session.document.revision,
                    "native_visible": frame.winit_window().and_then(|w| w.is_visible()),
                    "previous_frame_cpu_seconds": frame.info().cpu_usage,
                })
            },
        );
        // #endregion
        for message in self.logger.drain() {
            let _ = crate::emit(&message);
        }
        // protocol_error 必须是本帧最后一帧：宿主收到即判死连接，
        // 排在其后的帧不再被消费。
        if let Some(message) = self.pending_protocol_error.take() {
            let _ = crate::emit(&message);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // #region debug-point B:C:frame
        let debug_frame_gap_us = debug_ink::frame_gap();
        let debug_ui_start = debug_ink::start();
        // #endregion
        let ctx = ui.ctx().clone();
        self.board_ui(ui, &ctx);
        // #region debug-point B:C:frame
        let debug_ui_us = debug_ink::micros(debug_ui_start);
        debug_ink::sample(
            2,
            "B,C",
            "gui.rs:ui",
            ["ui", "frame_gap", "", ""],
            [debug_ui_us, debug_frame_gap_us, 0, 0],
            || {
                serde_json::json!({
                    "page": debug_ink::counts(self.session.document.current_page()),
                    "gesture_active": self.gesture.is_some(), "revision": self.session.document.revision,
                    "clear_alpha": <Self as eframe::App>::clear_color(self, ui.visuals())[3],
                    "window_fill_alpha": ui.visuals().window_fill.a(),
                    "panel_fill_alpha": ui.visuals().panel_fill.a(),
                    "scope": "app_ui_cpu_not_gpu_present",
                })
            },
        );
        // #endregion
    }
}

impl BoardApp {
    // The native logic phase and headless frame tests share this exact preparation
    // path. It may submit HWR, but visible results are accepted only after UI input.
    pub(super) fn prepare_workers(&mut self, ctx: &egui::Context, now: Instant) {
        let context = ContextToken::capture(&self.session.document);
        if !self.session.effective_visible()
            || self.compact_entry()
            || self.allow_close
            || self.ink_context.as_ref().is_some_and(|old| old != &context)
        {
            self.cancel_ink();
        }
        if !self.session.effective_visible() || self.compact_entry() || self.allow_close {
            self.finish_worker_frame(ctx);
            return;
        }
        if !self.confirm_close {
            self.poll_backend_ink(ctx, now);
        }
    }

    pub(super) fn board_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.board_ui_input(ui, ctx);
        self.finish_worker_frame(ctx);
    }

    pub(super) fn finish_worker_frame(&mut self, ctx: &egui::Context) {
        self.sync_handwriting_context();
        if self.handwriting.poll_after_ui() {
            self.cancel_math();
        }
        // egui consumes keyboard events while building widgets. Poll unconditionally
        // afterwards, not behind an input gate that can starve on held keys/pointers.
        self.poll_workers(ctx);
    }

    pub(super) fn board_ui_input(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.sync_handwriting_context();
        self.enforce_tool_mode();
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            self.request_close(ctx);
        }
        if !self.session.effective_visible() {
            return;
        }
        if self.compact_entry() {
            self.entry(ctx);
            return;
        }
        self.shortcuts(ctx);
        let previous_tool = self.tool;
        self.toolbar(ctx);
        if self.tool != previous_tool {
            self.gesture = None;
            self.touch_gesture = None;
            self.cancel_ink();
            self.swipe = None;
            self.swipe_touches.clear();
            self.pinch = None;
            self.select_touches.clear();
            self.view_drag = None;
            self.fling = egui::Vec2::ZERO;
        }
        if self.compact_entry() {
            return;
        }
        self.panels(ctx);
        self.authorization_panel(ctx);
        self.plot_controls(ctx);
        self.canvas(ui);
    }
}
