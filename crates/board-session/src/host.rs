use crate::*;

#[derive(Debug)]
pub(crate) struct Download {
    pub asset_ref: String,
    pub upload_id: String,
    pub request_id: String,
    offset: usize,
    total: usize,
}

impl Session {
    pub fn set_owned_windows(&mut self, window_ids: &[&str]) -> Result<Vec<Message>> {
        if window_ids.is_empty()
            || window_ids.len() > 128
            || window_ids.iter().any(|s| s.is_empty() || s.len() > 128)
        {
            return Err(error("invalid_params", "窗口列表必须非空、唯一且有界"));
        }
        let ids: HashSet<String> = window_ids.iter().map(|s| (*s).to_owned()).collect();
        if ids.len() != window_ids.len() {
            return Err(error("invalid_params", "窗口列表必须非空、唯一且有界"));
        }
        let mut out = Vec::new();
        let captures: Vec<_> = self
            .jobs
            .iter()
            .filter(|(_, j)| j.method == "host.capture_region")
            .map(|(id, _)| id.clone())
            .collect();
        for id in captures {
            self.cancel_job(&id, "window_set_changed", &mut out);
        }
        self.owned_windows = ids;
        self.has_window = true;
        let pending = self.pending_window_response.take();
        self.window_request = None;
        self.request_window(pending, &mut out);
        Ok(out)
    }

    pub fn acknowledge_windows(
        &mut self,
        request_id: &str,
        observations: &[(&str, bool)],
    ) -> Vec<Message> {
        let Some(request) = self.window_request.as_ref() else {
            return Vec::new();
        };
        if request.request_id != request_id || observations.len() != self.owned_windows.len() {
            return Vec::new();
        }
        let ids: HashSet<_> = observations.iter().map(|(id, _)| *id).collect();
        if ids.len() != observations.len()
            || ids.len() != self.owned_windows.len()
            || !ids.iter().all(|id| self.owned_windows.contains(*id))
            || observations
                .iter()
                .any(|(_, visible)| *visible != request.visible)
        {
            return Vec::new();
        }
        self.acknowledge_window(request_id, request.visible)
    }

    pub fn save_document(&mut self, path: impl AsRef<std::path::Path>) -> Result<()> {
        package::save(&self.document, &self.resources, path.as_ref())?;
        self.history.mark_saved(&self.document);
        Ok(())
    }

    pub fn open_document(
        &mut self,
        path: impl AsRef<std::path::Path>,
        discard_unsaved: bool,
    ) -> Result<Vec<Message>> {
        if self.closed || self.close_pending {
            return Err(error("session_closed", "会话已关闭或正在关闭"));
        }
        self.guard_discard(&json!({"discard_unsaved": discard_unsaved}))?;
        let (document, resources) = package::load(path.as_ref())?;
        let mut out = Vec::new();
        self.replace_document(document, &mut out);
        self.resources = resources;
        out.push(event("document_changed", self.state()));
        out.push(event("state_changed", self.state()));
        Ok(out)
    }

    // EOF 本身不等于截图进程已停止，保留未确认的截图租约；不丢弃文档或资源。
    pub fn host_disconnected(&mut self) -> Vec<Message> {
        if !self.connected {
            return Vec::new();
        }
        self.connected = false;
        let mut out = Vec::new();
        let ids: Vec<_> = self.jobs.keys().cloned().collect();
        for id in ids {
            self.cancel_job(&id, "host_disconnected", &mut out);
        }
        self.resources.abort_uploads();
        self.hide_leases.retain(|id| id.starts_with("capture:"));
        if let Some(id) = self.pending_window_response.take() {
            out.push(response(&id, Err(error("host_disconnected", "主机已断开"))));
        }
        self.request_window(None, &mut out);
        out.retain(|message| !matches!(message, Message::Request(_)));
        out.push(event("state_changed", self.state()));
        out
    }

    // 仅在 GUI/宿主适配器已确认采集进程停止后调用，不能将普通 EOF 当作确认。
    pub fn confirm_host_stopped(&mut self) -> Vec<Message> {
        if self.connected {
            return Vec::new();
        }
        for (_, cancelled) in self.cancelled_captures.drain() {
            self.hide_leases.remove(&cancelled.lease);
        }
        let mut out = Vec::new();
        self.request_window(None, &mut out);
        out.push(event("state_changed", self.state()));
        out
    }

    fn job_current(&self, job: &Job) -> Result<()> {
        if self.closed
            || self.close_pending
            || self.document.id != job.document_id
            || self.document.revision != job.revision
            || !self.document.pages.iter().any(|p| p.id == job.page_id)
        {
            return Err(error("revision_conflict", "任务对应文档、页面或版本已变化"));
        }
        if job.method == "host.capture_region" {
            if self.permissions.classroom_safe || !self.permissions.desktop_capture_allowed {
                return Err(error("permission_denied", "截图权限已撤销"));
            }
        } else if !self.permissions.agent_allowed {
            return Err(error("permission_denied", "Agent 权限已撤销"));
        }
        Ok(())
    }

    fn host_asset<'a>(&self, value: &'a Value) -> Option<&'a str> {
        let asset = value.get("asset_ref")?.as_str()?;
        // 描述符属于宿主资源命名空间；同名本地资源不能替代下载或免除清理。
        (value.get("total_bytes").is_some()
            || value.get("crc32").is_some()
            || self.resources.get(asset).is_none())
        .then_some(asset)
    }

    pub(crate) fn release_host_resource(&self, asset_ref: &str, out: &mut Vec<Message>) {
        if self.connected && resources::valid_asset_ref(asset_ref) {
            out.push(
                Request::new(
                    format!("runtime:{}", new_id()),
                    "resources.release",
                    json!({"asset_ref": asset_ref}),
                )
                .unwrap()
                .into(),
            );
        }
    }

    fn stopped_capture(&mut self, id: &str) -> Vec<Message> {
        let cancelled = self.cancelled_captures.remove(id).unwrap();
        self.hide_leases.remove(&cancelled.lease);
        let mut out = Vec::new();
        self.request_window(None, &mut out);
        out.push(event("state_changed", self.state()));
        self.dispatch_jobs(&mut out);
        out
    }

    pub(crate) fn process_host_response(&mut self, incoming: Response) -> Vec<Message> {
        if !self.connected {
            return Vec::new();
        }
        let has_pending_id = incoming
            .result
            .as_ref()
            .is_some_and(|v| v.get("job_id").is_some());
        let pending_id = incoming
            .result
            .as_ref()
            .and_then(|v| v.get("job_id"))
            .and_then(Value::as_str);
        if let Some(id) = self
            .cancelled_captures
            .iter()
            .find(|(_, c)| c.request_id == incoming.id && c.host_job_id.is_none())
            .map(|(id, _)| id.clone())
        {
            if incoming.ok && has_pending_id {
                let Some(host_id) = pending_id else {
                    return Vec::new();
                };
                if host_id.is_empty()
                    || host_id.len() > 256
                    || self.seen_host_jobs.contains(host_id)
                    || self.seen_host_jobs.len() >= 4096
                {
                    return Vec::new();
                }
                self.seen_host_jobs.insert(host_id.into());
                // 新一轮取消必须使用新 ID，旧响应不能解除新一轮等待的隐藏租约。
                let mut cancelled = self.cancelled_captures.remove(&id).unwrap();
                cancelled.host_job_id = Some(host_id.into());
                let cancel_id = format!("runtime:{}", new_id());
                self.cancelled_captures.insert(cancel_id.clone(), cancelled);
                return vec![
                    Request::new(
                        &cancel_id,
                        "jobs.cancel",
                        json!({"job_id": host_id, "request_id": incoming.id}),
                    )
                    .unwrap()
                    .into(),
                ];
            }
            let mut out = self.stopped_capture(&id);
            if let Some(asset) = incoming
                .result
                .as_ref()
                .and_then(|value| self.host_asset(value))
            {
                self.release_host_resource(asset, &mut out);
            }
            return out;
        }
        if let Some(cancelled) = self.cancelled_captures.get(&incoming.id) {
            if incoming.ok
                && incoming.result.as_ref().is_some_and(|v| {
                    v["cancelled"] == true
                        && v.get("job_id").is_none_or(|id| {
                            id.as_str()
                                == Some(
                                    cancelled
                                        .host_job_id
                                        .as_deref()
                                        .unwrap_or(&cancelled.job_id),
                                )
                        })
                })
            {
                return self.stopped_capture(&incoming.id);
            }
            return Vec::new();
        }
        if let Some(id) = self
            .jobs
            .iter()
            .find(|(_, j)| {
                j.download
                    .as_ref()
                    .is_some_and(|d| d.request_id == incoming.id)
            })
            .map(|(id, _)| id.clone())
        {
            return self.download_response(id, incoming);
        }
        let Some(id) = self
            .jobs
            .iter()
            .find(|(_, j)| {
                j.request_id.as_deref() == Some(&incoming.id)
                    && j.host_job_id.is_none()
                    && j.download.is_none()
            })
            .map(|(id, _)| id.clone())
        else {
            return Vec::new();
        };
        if incoming.ok && has_pending_id && pending_id.is_none() {
            let mut out = Vec::new();
            self.cancel_job(&id, "invalid_host_response", &mut out);
            out.push(event("state_changed", self.state()));
            return out;
        }
        if incoming.ok
            && let Some(host_id) = pending_id
        {
            if host_id.is_empty()
                || host_id.len() > 256
                || self.seen_host_jobs.contains(host_id)
                || self.seen_host_jobs.len() >= 4096
            {
                let mut out = Vec::new();
                self.cancel_job(&id, "invalid_host_response", &mut out);
                out.push(event("state_changed", self.state()));
                return out;
            }
            self.seen_host_jobs.insert(host_id.into());
            self.jobs.get_mut(&id).unwrap().host_job_id = Some(host_id.into());
            return vec![event("state_changed", self.state())];
        }
        let result = if incoming.ok {
            Ok(incoming.result.unwrap())
        } else {
            Err(incoming.error.unwrap())
        };
        self.finish_host_job(&id, result)
    }

    // 接受单个最终事件；jobs.finished 是兼容别名，不是批量数组。
    pub fn handle_event(&mut self, incoming: Event) -> Vec<Message> {
        if !self.connected
            || !matches!(incoming.event.as_str(), "job.finished" | "jobs.finished")
            || Message::Event(incoming.clone()).validate().is_err()
        {
            return Vec::new();
        }
        let data = &incoming.data;
        let Ok(host_id) = text(data, "job_id") else {
            return Vec::new();
        };
        let request_id = match data.get("request_id") {
            None => None,
            Some(Value::String(id)) => Some(id.as_str()),
            Some(_) => return Vec::new(),
        };
        if serde_json::to_vec(&incoming).map_or(true, |v| v.len() > MAX_LINE_BYTES) {
            let id = self
                .jobs
                .iter()
                .find(|(id, j)| {
                    j.download.is_none()
                        && j.request_id.is_some()
                        && (j.host_job_id.as_deref() == Some(host_id)
                            || (j.host_job_id.is_none()
                                && id.as_str() == host_id
                                && request_id == j.request_id.as_deref()))
                        && request_id.is_none_or(|r| Some(r) == j.request_id.as_deref())
                })
                .map(|(id, _)| id.clone());
            let mut out = Vec::new();
            if let Some(id) = id {
                self.cancel_job(&id, "response_too_large", &mut out);
                out.push(event("state_changed", self.state()));
            }
            return out;
        }
        let result = match data["ok"].as_bool() {
            Some(true) if data.get("result").is_some() && data.get("error").is_none() => {
                Ok(data["result"].clone())
            }
            Some(false) if data.get("result").is_none() => {
                match decode::<ProtocolError>(data["error"].clone()) {
                    Ok(e) => Err(e),
                    Err(_) => return Vec::new(),
                }
            }
            _ => return Vec::new(),
        };
        if let Some(id) = self
            .cancelled_captures
            .iter()
            .find(|(_, c)| {
                (c.host_job_id.as_deref() == Some(host_id)
                    || (c.host_job_id.is_none()
                        && c.job_id == host_id
                        && request_id == Some(c.request_id.as_str())))
                    && request_id.is_none_or(|r| r == c.request_id)
            })
            .map(|(id, _)| id.clone())
        {
            let mut out = self.stopped_capture(&id);
            if let Ok(value) = result
                && let Some(asset) = self.host_asset(&value)
            {
                self.release_host_resource(asset, &mut out);
            }
            return out;
        }
        let Some(id) = self
            .jobs
            .iter()
            .find(|(id, j)| {
                j.download.is_none()
                    && j.request_id.is_some()
                    && (j.host_job_id.as_deref() == Some(host_id)
                        || (j.host_job_id.is_none()
                            && id.as_str() == host_id
                            && request_id == j.request_id.as_deref()))
                    && request_id.is_none_or(|r| Some(r) == j.request_id.as_deref())
            })
            .map(|(id, _)| id.clone())
        else {
            return Vec::new();
        };
        self.finish_host_job(&id, result)
    }

    fn request_download(download: &mut Download, out: &mut Vec<Message>) {
        download.request_id = format!("runtime:{}", new_id());
        out.push(Request::new(&download.request_id, "resources.read", json!({"asset_ref": download.asset_ref,
            "offset": download.offset, "length": resources::MAX_READ_BYTES.min(download.total - download.offset)})).unwrap().into());
    }

    fn finish_host_job(&mut self, id: &str, incoming: Result<Value>) -> Vec<Message> {
        let mut job = self.jobs.remove(id).unwrap();
        let mut out = Vec::new();
        let before = self.document.revision;
        let incoming = match self.job_current(&job) {
            Ok(()) => incoming,
            Err(e) => {
                if job.method == "host.capture_region"
                    && let Ok(value) = &incoming
                    && let Some(asset) = self.host_asset(value)
                {
                    self.release_host_resource(asset, &mut out);
                }
                Err(e)
            }
        };
        let result = match incoming {
            Ok(value)
                if job.method == "host.capture_region" && self.host_asset(&value).is_some() =>
            {
                let asset = value["asset_ref"].as_str().unwrap();
                let setup = (|| {
                    if !resources::valid_asset_ref(asset) {
                        return Err(error("invalid_resource", "主机资源引用无效"));
                    }
                    let total = usize::try_from(number(&value, "total_bytes")?)
                        .map_err(|_| error("resource_limit", "资源过大"))?;
                    let crc = u32::try_from(number(&value, "crc32")?)
                        .map_err(|_| error("invalid_params", "CRC32 无效"))?;
                    let upload_id = self.resources.begin_upload(id, total, crc)?;
                    Ok(Download {
                        asset_ref: asset.into(),
                        upload_id,
                        request_id: String::new(),
                        offset: 0,
                        total,
                    })
                })();
                match setup {
                    Ok(mut download) => {
                        // 已收到截图最终完成，后续只读资源，不再需要隐藏。
                        self.release_job_lease(&job, &mut out);
                        job.lease = None;
                        Self::request_download(&mut download, &mut out);
                        job.download = Some(download);
                        self.jobs.insert(id.into(), job);
                        out.push(event("state_changed", self.state()));
                        return out;
                    }
                    Err(e) => {
                        self.release_host_resource(asset, &mut out);
                        Err(e)
                    }
                }
            }
            Ok(value) => self.complete_job(&job, value),
            Err(e) => Err(e),
        };
        self.release_job_lease(&job, &mut out);
        self.emit_finished(id, result, &mut out);
        if before != self.document.revision {
            out.push(event("document_changed", self.state()));
        }
        out.push(event("state_changed", self.state()));
        self.dispatch_jobs(&mut out);
        out
    }

    fn emit_finished(&self, id: &str, result: Result<Value>, out: &mut Vec<Message>) {
        let data = match result {
            Ok(result) => json!({"job_id": id, "ok": true, "result": result}),
            Err(e) => json!({"job_id": id, "ok": false, "error": e}),
        };
        let finished: Message = Event::new("job.finished", data).into();
        if serde_json::to_vec(&finished).is_ok_and(|v| v.len() <= MAX_LINE_BYTES) {
            out.push(finished);
        } else {
            out.push(event("job.finished", json!({"job_id": id, "ok": false, "error": error("response_too_large", "任务结果过长")})));
        }
    }

    fn download_response(&mut self, id: String, incoming: Response) -> Vec<Message> {
        let mut job = self.jobs.remove(&id).unwrap();
        let mut download = job.download.take().unwrap();
        let mut out = Vec::new();
        let result = (|| {
            self.job_current(&job)?;
            if !incoming.ok {
                return Err(incoming.error.unwrap());
            }
            let value = incoming.result.unwrap();
            let bytes: Vec<u8> = decode(value["bytes"].clone())?;
            if text(&value, "asset_ref")? != download.asset_ref
                || number(&value, "offset")? != download.offset as u64
                || number(&value, "total_bytes")? != download.total as u64
            {
                return Err(error("invalid_host_response", "资源读取上下文不匹配"));
            }
            let next =
                self.resources
                    .upload_chunk(&id, &download.upload_id, download.offset, &bytes)?;
            if number(&value, "next_offset")? != next as u64
                || value["eof"].as_bool() != Some(next == download.total)
            {
                return Err(error("invalid_host_response", "资源读取进度无效"));
            }
            download.offset = next;
            if next == download.total {
                let asset = self.resources.finish_upload(&id, &download.upload_id)?;
                let resource = self.resources.get(&asset).unwrap();
                Ok(Some(
                    json!({"asset_ref": asset, "mime_type": "image/png", "width": resource.width, "height": resource.height}),
                ))
            } else {
                Ok(None)
            }
        })();
        match result {
            Ok(None) => {
                Self::request_download(&mut download, &mut out);
                job.download = Some(download);
                self.jobs.insert(id, job);
                return out;
            }
            Ok(Some(value)) => self.emit_finished(&id, Ok(value), &mut out),
            Err(e) => {
                let _ = self.resources.abort_upload(&id, &download.upload_id);
                self.emit_finished(&id, Err(e), &mut out);
            }
        }
        self.release_host_resource(&download.asset_ref, &mut out);
        out.push(event("state_changed", self.state()));
        out
    }
}
