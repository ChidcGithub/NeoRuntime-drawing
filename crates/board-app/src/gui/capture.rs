use super::*;

use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const LOCAL_HINT: &str = "点击授权本次内置框选截图；拖动选择区域，Esc 或右键取消。按隐藏选项处理窗口，不调用系统截图工具，不访问剪贴板，不上传。截图结束后自动恢复本次窗口状态。";

enum Phase {
    Waiting { hidden_round: bool },
    Running(mpsc::Receiver<std::result::Result<Vec<u8>, String>>),
}

pub(super) struct LocalCapture {
    context: CaptureContext,
    lease: Option<String>,
    started: Instant,
    cancel: Arc<AtomicBool>,
    phase: Phase,
}

impl Drop for LocalCapture {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

impl BoardApp {
    pub(super) fn screenshot_clicked(&mut self) {
        self.cancel_authorization();
        if self.hosted {
            // GUI click authorizes this request, not the session's RPC permissions.
            self.request_host(true);
        } else if !crate::local_capture::supported() {
            self.status = "本地内置框选仅支持 Windows；未采集".into();
        } else {
            self.begin_local_capture();
        }
    }

    pub(super) fn begin_local_capture(&mut self) {
        if self.local_capture.is_some() {
            self.status = "本次截图尚未结束，请完成框选或恢复确认".into();
            return;
        }
        let state = self.session.state();
        if self.hosted
            || !self.session.effective_visible()
            || state["has_window"] != true
            || state["owned_window_count"] != 1
        {
            self.status = "当前窗口状态不允许本地截图；未采集".into();
            return;
        }
        let lease = self
            .capture_hide_window
            .then(|| format!("gui-local:{}", new_id()));
        if let Some(lease) = &lease {
            let request = Request::new(
                format!("runtime:gui:{}", new_id()),
                "window.suspend",
                serde_json::json!({"lease_id": lease}),
            )
            .unwrap();
            let out = self.session.handle(request);
            let rejected = out
                .iter()
                .any(|m| matches!(m, Message::Response(r) if !r.ok));
            self.messages(out);
            if rejected {
                return;
            }
        }
        self.gesture = None;
        self.split_drag = None;
        self.cancel_ink();
        self.local_capture = Some(LocalCapture {
            context: CaptureContext {
                context: ContextToken::capture(&self.session.document),
                canvas_size: self.canvas_size,
                stale: false,
            },
            lease,
            started: Instant::now(),
            cancel: Arc::new(AtomicBool::new(false)),
            phase: Phase::Waiting {
                hidden_round: false,
            },
        });
        self.status = if self.capture_hide_window {
            "正在隐藏板书；内置框选完成后自动上板，不上传"
        } else {
            "保留窗口截图：窗口和笔迹可能进入截图；完成后自动上板，不上传"
        }
        .into();
    }

    pub(super) fn invalidate_local_capture(&mut self, replaced: bool) {
        if let Some(local) = &mut self.local_capture {
            local.context.stale |=
                replaced || local.context.context != ContextToken::capture(&self.session.document);
        }
    }

    pub(super) fn cancel_local_capture(&mut self) {
        if let Some(local) = &self.local_capture {
            local.cancel.store(true, Ordering::Release);
        }
    }

    // Wait one extra logic round for menus to close in both modes. Hidden mode
    // additionally requires native hide acknowledgement; UI never launches capture.
    pub(super) fn poll_local_capture(&mut self) {
        if self.allow_close || self.session.closed || self.session.state()["close_pending"] == true
        {
            self.cancel_local_capture();
            return;
        }
        self.invalidate_local_capture(false);
        if self.local_capture.as_ref().is_some_and(|local| {
            matches!(local.phase, Phase::Waiting { .. })
                && (local.started.elapsed() >= Duration::from_secs(5)
                    || local.cancel.load(Ordering::Acquire))
        }) {
            let local = self.local_capture.take().unwrap();
            self.status = "窗口状态未就绪或请求已取消；没有启动内置截图".into();
            self.release_local_lease(&local.lease);
            return;
        }
        let state = self.session.state();
        let Some(local) = &mut self.local_capture else {
            return;
        };
        match &mut local.phase {
            Phase::Waiting { hidden_round } => {
                if (local.lease.is_some() && state["hidden_confirmed"] != true)
                    || (local.lease.is_none()
                        && (!self.session.effective_visible() || state["visible"] != true))
                    || state["owned_window_count"] != 1
                    || self.session.pending_window_request().is_some()
                {
                    *hidden_round = false;
                    return;
                }
                if !*hidden_round {
                    *hidden_round = true;
                    return;
                }
                let (tx, rx) = mpsc::channel();
                let cancel = local.cancel.clone();
                local.phase = Phase::Running(rx);
                if let Err(error) = std::thread::Builder::new()
                    .name("local-capture".into())
                    .spawn(move || {
                        let result = std::panic::catch_unwind(|| {
                            flush_compositor()?;
                            if cancel.load(Ordering::Acquire) {
                                return Err("截图已取消".into());
                            }
                            crate::local_capture::capture(cancel)
                        })
                        .unwrap_or_else(|_| Err("截图线程异常终止".into()));
                        let _ = tx.send(result);
                    })
                {
                    self.finish_local_capture(Err(format!("无法启动截图线程：{error}")));
                }
            }
            Phase::Running(rx) => {
                let result = match rx.try_recv() {
                    Ok(result) => result,
                    Err(mpsc::TryRecvError::Empty) => return,
                    Err(mpsc::TryRecvError::Disconnected) => Err("截图线程已断开".into()),
                };
                self.finish_local_capture(result);
            }
        }
    }

    pub(super) fn finish_local_capture(&mut self, result: std::result::Result<Vec<u8>, String>) {
        let Some(local) = self.local_capture.take() else {
            return;
        };
        // The owned selector has exited before the worker publishes its result.
        // Cancellation never restores the board while pixel acquisition is still running.
        if local.cancel.load(Ordering::Acquire)
            || self.allow_close
            || self.session.closed
            || self.session.state()["close_pending"] == true
        {
            self.status = "截图已取消；未插入或上传".into();
        } else {
            match result {
                Ok(bytes)
                    if !local.context.stale
                        && local.context.context
                            == ContextToken::capture(&self.session.document) =>
                {
                    match self.session.resources.import_png(bytes) {
                        Ok(asset) => self.insert_capture(
                            CaptureContext {
                                context: local.context.context.clone(),
                                canvas_size: local.context.canvas_size,
                                stale: false,
                            },
                            asset,
                        ),
                        Err(error) => self.status = format!("截图图片导入失败：{error:?}"),
                    }
                }
                Ok(_) => {
                    self.status = "原文档/页面/版本已变化，已丢弃本地截图；未导入或上传".into()
                }
                Err(error) => self.status = error,
            }
        }
        self.release_local_lease(&local.lease);
    }

    fn release_local_lease(&mut self, lease: &Option<String>) {
        let Some(lease) = lease else {
            return;
        };
        let status = self.status.clone();
        let out = self.session.handle(
            Request::new(
                format!("runtime:gui:{}", new_id()),
                "window.resume",
                serde_json::json!({"lease_id": lease}),
            )
            .unwrap(),
        );
        self.messages(out);
        self.status = status;
    }

    #[cfg(test)]
    pub(super) fn local_capture_test_state(&self) -> (&'static str, Arc<AtomicBool>) {
        let local = self.local_capture.as_ref().unwrap();
        (
            match local.phase {
                Phase::Waiting {
                    hidden_round: false,
                } => "waiting",
                Phase::Waiting { hidden_round: true } => "armed",
                Phase::Running(_) => "running",
            },
            local.cancel.clone(),
        )
    }

    pub(super) fn open_image_agent(&mut self, object: &BoardObject) {
        let ObjectKind::Image { asset_ref, .. } = &object.kind else {
            return;
        };
        self.cancel_authorization();
        self.agent_image = Some((
            ContextToken::capture(&self.session.document),
            object.id.clone(),
            asset_ref.clone(),
        ));
        self.authorization = Some(false);
        self.gesture = None;
    }

    pub(super) fn clicked_image(&self, pos: Pos2) -> Option<BoardObject> {
        let gesture = self.gesture.as_ref()?;
        if self.tool != Tool::Select
            || gesture.vertex.is_some()
            || gesture.resize.is_some()
            || gesture.document != self.session.document.id
            || gesture.page != self.session.document.current_page().id
            || gesture.revision != self.session.document.revision
        {
            return None;
        }
        let object = self
            .session
            .document
            .current_page()
            .objects
            .iter()
            .rev()
            .find(|object| editing::hit_test(object, pos, 8.0))?;
        if !matches!(object.kind, ObjectKind::Image { .. })
            || gesture.original.as_ref() != Some(object)
        {
            return None;
        }
        Some(object.clone())
    }
}

#[cfg(all(windows, not(test)))]
fn flush_compositor() -> std::result::Result<(), String> {
    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmFlush() -> i32;
    }
    // SAFETY: DwmFlush takes no pointers; a failed barrier must not launch capture.
    let status = unsafe { DwmFlush() };
    if status < 0 {
        Err(format!("等待窗口合成失败：{status:#x}"))
    } else {
        Ok(())
    }
}

#[cfg(any(not(windows), test))]
fn flush_compositor() -> std::result::Result<(), String> {
    Ok(())
}
