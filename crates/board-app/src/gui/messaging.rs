// 宿主消息接入：入站分发、捕获租约、断连清理与动态取色请求处理。
// 从 gui.rs 拆出（纯移动，行为不变）。
use super::*;

impl BoardApp {
    pub(super) fn messages(&mut self, messages: Vec<Message>) {
        self.invalidate_captures();
        let mut captures = Vec::new();
        for message in messages {
            match &message {
                Message::Response(response) if response.id.starts_with("runtime:gui:") => {
                    if response.ok
                        && let Some(result) = &response.result
                        && result["status"] == "pending"
                        && let Some(id) = result["job_id"].as_str()
                    {
                        self.jobs.push((id.to_owned(), Instant::now()));
                        let kind = if self.capture_requests.contains_key(&response.id) {
                            "capture"
                        } else {
                            "agent"
                        };
                        self.logger
                            .debug("host job accepted", &[("job_id", id), ("kind", kind)]);
                    }
                    if let Some(capture) = self.capture_requests.remove(&response.id)
                        && response.ok
                        && let Some(id) =
                            response.result.as_ref().and_then(|r| r["job_id"].as_str())
                    {
                        self.capture_jobs.insert(id.to_owned(), capture);
                    }
                    self.status = if response.ok {
                        format!(
                            "请求已受理：{}",
                            response.result.as_ref().unwrap_or(&serde_json::Value::Null)
                        )
                    } else {
                        format!("请求被拒绝：{:?}", response.error)
                    };
                }
                Message::Event(event) if event.event == "document_changed" => {
                    // The renderer synchronizes changed/deleted objects by content and revision.
                    // Keep mathematical samples across move/resize commits and undo/redo;
                    // document replacement paths explicitly clear even for the same document ID.
                    self.thumbnails.clear();
                    self.textures.clear();
                    self.plot_worker.cancel();
                    self.plot_points = None;
                    // 多指手势仅在文档/页面被替换时清空；自身提交推进的 revision
                    // 由提交点的同步保持在书手势有效。
                    self.multi_gestures.retain(|(_, gesture)| {
                        gesture.document == self.session.document.id
                            && gesture.page == self.session.document.current_page().id
                    });
                    self.cancel_authorization();
                }
                Message::Event(event) if event.event == "job.finished" => {
                    self.jobs
                        .retain(|(id, _)| Some(id.as_str()) != event.data["job_id"].as_str());
                    let kind = event.data["job_id"]
                        .as_str()
                        .map(|id| {
                            if self.capture_jobs.contains_key(id) {
                                "capture"
                            } else {
                                "agent"
                            }
                        })
                        .unwrap_or("agent");
                    let (level, outcome) = if event.data["ok"] == true {
                        (crate::board_log::Level::Info, "ok")
                    } else if event.data["error"]["code"] == "cancelled" {
                        (crate::board_log::Level::Info, "cancelled")
                    } else {
                        (crate::board_log::Level::Warn, "failed")
                    };
                    self.logger.log(
                        level,
                        "host job finished",
                        &[
                            ("job_id", event.data["job_id"].as_str().unwrap_or("")),
                            ("kind", kind),
                            ("outcome", outcome),
                        ],
                    );
                    self.status = format!("任务响应：{}", event.data);
                    if let Some(capture) = event.data["job_id"]
                        .as_str()
                        .and_then(|id| self.capture_jobs.remove(id))
                        && event.data["ok"] == true
                        && let Some(asset) = event.data["result"]["asset_ref"].as_str()
                        && self.session.resources.get(asset).is_some()
                    {
                        captures.push((capture, asset.to_owned()));
                    }
                }
                _ => {}
            }
            #[cfg(test)]
            if !matches!(&message, Message::Response(r) if r.id.starts_with("runtime:gui:")) {
                self.emitted.push(message.clone());
            }
            if self.hosted
                && !matches!(&message, Message::Response(r) if r.id.starts_with("runtime:gui:"))
                && let Err(error) = emit(&message)
            {
                eprintln!("协议输出失败：{error}");
                self.disconnected();
            }
        }
        // Finish emitting the host batch before changing revision: never interleave newer
        // document events with the session's older response/state_changed snapshots.
        for (capture, asset) in captures {
            self.insert_capture(capture, asset);
        }
    }

    pub(super) fn invalidate_captures(&mut self) {
        self.invalidate_local_capture(false);
        let context = ContextToken::capture(&self.session.document);
        for capture in self
            .capture_jobs
            .values_mut()
            .chain(self.capture_requests.values_mut())
        {
            capture.stale |= capture.context != context;
        }
    }

    pub(super) fn insert_capture(&mut self, capture: CaptureContext, asset_ref: String) {
        self.captured_asset = Some(asset_ref.clone());
        if capture.stale || capture.context != ContextToken::capture(&self.session.document) {
            self.status =
                "截图已返回，但原页面/版本已变化；请在文件面板手动插入截图（不上传）".into();
            return;
        }
        let Some(resource) = self.session.resources.get(&asset_ref) else {
            return;
        };
        let area = (capture.canvas_size - Vec2::splat(80.0)).max(Vec2::splat(1.0));
        let factor = (area.x / resource.width as f32)
            .min(area.y / resource.height as f32)
            .min(1.0);
        let width = resource.width as f32 * factor;
        let height = resource.height as f32 * factor;
        let object = BoardObject {
            id: new_id(),
            kind: ObjectKind::Image {
                position: Point {
                    x: ((capture.canvas_size.x - width) / 2.0).max(0.0),
                    y: ((capture.canvas_size.y - height) / 2.0).max(0.0),
                },
                width,
                height,
                asset_ref,
            },
        };
        let image_id = object.id.clone();
        let s = &mut self.session;
        let page = s.document.current_page().id.clone();
        let revision = s.document.revision;
        match s.history.apply(
            &mut s.document,
            &page,
            revision,
            &[Operation::Add { object }],
        ) {
            Ok(()) => {
                self.captured_asset = None;
                self.gesture = None;
                self.split_drag = None;
                self.changed();
                self.tool = Tool::Select;
                self.selected = Some(image_id);
                self.status = "截图已插入并选中；单击图片可询问 Agent，拖动可移动，可一次撤销；未发送给 Agent".into();
            }
            Err(error) => self.status = format!("截图自动插入失败：{error}；可在文件面板手动插入"),
        }
    }

    pub(super) fn disconnected(&mut self) {
        self.hosted = false;
        self.incoming = None;
        self.session.host_disconnected();
        self.capture_requests.clear();
        self.capture_jobs.clear();
        self.jobs.clear();
        self.cancel_authorization();
        self.plot_worker.cancel();
        self.plot_points = None;
        self.gesture = None;
        self.cancel_ink();
        self.status = "宿主已断开：可本地保存/退出；未确认停止的截图仍保持隐藏".into();
        self.logger.warn("host disconnected", &[]);
        match crate::recover_document(&mut self.session, &crate::recovery_directory()) {
            Ok(path) => {
                if let Some(path) = path {
                    self.logger.warn("recovery saved", &[]);
                    self.status
                        .push_str(&format!("；恢复包：{}", path.display()));
                }
                // 隐藏租约期间绝不显示板书/确认窗口；保存恢复包后可安全退出本 app。
                if !self.session.effective_visible() {
                    self.allow_close = true;
                }
            }
            Err(e) => {
                self.logger.error("recovery failed", &[]);
                self.status.push_str(&format!("；恢复失败：{e}"));
                eprintln!("{}", self.status);
            }
        }
    }
    /// 动态取色：theme.seed（Neo -> 子项目）。seed 为 "#RRGGBB" 或 "RRGGBB"；
    /// 双主题槽（跟随系统明暗）即时从种子重生成，密度与触摸距离不变。
    /// 不持久化：宿主重启后需重新发送。画布笔迹与用户内容不受影响。
    pub(super) fn handle_theme_seed(
        &mut self,
        ctx: &egui::Context,
        request: Request,
    ) -> Vec<Message> {
        let fail = |id: &str, code: &str, message: &str| {
            vec![Message::Response(
                Response::failure(id, ProtocolError::new(code, message)).unwrap(),
            )]
        };
        // 与文档会话同一套帧校验与生命周期门控。
        if let Err(e) = Message::Request(request.clone()).validate() {
            return vec![Message::Event(Event::new(
                "protocol_error",
                serde_json::json!({"code": e.code()}),
            ))];
        }
        if request.id.len() > 256 {
            return fail(&request.id, "invalid_id", "请求 ID 最多 256 字节");
        }
        if !self.session.configured {
            return fail(&request.id, "not_configured", "必须先 configure");
        }
        if self.session.state()["close_pending"] == true {
            return fail(&request.id, "session_closing", "等待全部窗口关闭确认");
        }
        if self.session.closed {
            return fail(&request.id, "session_closed", "会话已关闭");
        }
        if !request.params.is_object() {
            return fail(&request.id, "invalid_params", "params 必须是对象");
        }
        let Some(raw) = request.params["seed"].as_str() else {
            return fail(
                &request.id,
                "invalid_params",
                "seed 必须是 \"#RRGGBB\" 或 \"RRGGBB\" 字符串",
            );
        };
        let trimmed = raw.trim();
        let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return fail(
                &request.id,
                "invalid_params",
                "seed 必须是 6 位十六进制 RGB",
            );
        }
        let value = u32::from_str_radix(hex, 16).unwrap_or(0);
        let seed = Color32::from_rgb(
            ((value >> 16) & 0xFF) as u8,
            ((value >> 8) & 0xFF) as u8,
            (value & 0xFF) as u8,
        );
        apply_theme_seed(ctx, Some(seed));
        vec![Message::Response(
            Response::success(
                &request.id,
                serde_json::json!({
                    "applied": true,
                    "seed": format!("#{}", hex.to_ascii_lowercase()),
                    "mode": "both",
                }),
            )
            .unwrap(),
        )]
    }

    pub(super) fn incoming_message(&mut self, message: Message) {
        let before = ContextToken::capture(&self.session.document);
        let mut replaced = false;
        let mut capture_selection = None;
        match message {
            Message::Request(request) => {
                let replacing = matches!(request.method.as_str(), "document.open" | "document.new");
                let out = self.session.handle(request);
                replaced = replacing
                    && out.iter().any(
                        |message| matches!(message, Message::Response(response) if response.ok),
                    );
                self.messages(out);
            }
            Message::Response(response) => {
                let out = self.session.handle_response(response);
                let revision = self.session.document.revision;
                self.messages(out);
                if self.session.document.revision != revision {
                    capture_selection = self.selected.clone();
                }
            }
            Message::Event(event) if event.event == "protocol_error" => {
                // 宿主 transport 收到 protocol_error 会立刻判死连接：断连/恢复的
                // log 帧先行，protocol_error 在本帧最后发出。
                self.logger.error(
                    "transport fatal",
                    &[("kind", event.data["code"].as_str().unwrap_or("unknown"))],
                );
                self.pending_protocol_error = Some(event.into());
                if self.session.state()["pending_jobs"].as_u64().unwrap_or(0) > 0 {
                    self.disconnected();
                }
            }
            Message::Event(event) => {
                let out = self.session.handle_event(event);
                let revision = self.session.document.revision;
                self.messages(out);
                if self.session.document.revision != revision {
                    capture_selection = self.selected.clone();
                }
            }
        }
        self.invalidate_local_capture(replaced);
        if self.session.closed || self.session.state()["close_pending"] == true {
            self.cancel_local_capture();
        }
        if replaced {
            self.handwriting.invalidate_context();
            self.ink_cache.clear();
            self.textures.clear();
            self.renderer.clear();
            self.thumbnails.clear();
            self.captured_asset = None;
        }
        if replaced
            || before != ContextToken::capture(&self.session.document)
            || !self.session.effective_visible()
        {
            self.plot_worker.cancel();
            self.gesture = None;
            self.split_drag = None;
            // 文档被宿主变更：瞬态视图交互与手势一并取消。
            self.view_drag = None;
            self.pinch = None;
            self.select_touches.clear();
            self.fling = egui::Vec2::ZERO;
            self.swipe = None;
            self.swipe_touches.clear();
            self.touch_gesture = None;
            // Only messages()' successful capture insertion changes revision here;
            // preserve its new selection, not a selection predating the host result.
            self.selected = capture_selection;
            self.plot_points = None;
            self.cancel_authorization();
            self.cancel_ink();
        }
        self.sync_ink_cache();
    }
}
