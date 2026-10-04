//! Drawing 与 Blackboard 共用会话；默认无窗口，GUI 必须显式接入显隐确认。
mod host;
mod package;
mod resources;
pub use resources::{Resource, ResourceStore};

use board_core::{Connection, Document, History, ObjectKind, Operation, new_id};
use board_protocol::{Event, MAX_LINE_BYTES, Message, ProtocolError, Request, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

pub type Result<T> = std::result::Result<T, ProtocolError>;
fn error(code: &str, message: &str) -> ProtocolError {
    ProtocolError::new(code, message)
}
fn core_error(e: board_core::Error) -> ProtocolError {
    let code = match e {
        board_core::Error::RevisionConflict { .. } => "revision_conflict",
        board_core::Error::PageNotFound(_) => "page_not_found",
        board_core::Error::Io(_) => "io_error",
        _ => "invalid_document",
    };
    ProtocolError::new(code, e.to_string())
}
fn text<'a>(p: &'a Value, name: &str) -> Result<&'a str> {
    p.get(name)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| error("invalid_params", &format!("缺少非空字符串 {name}")))
}
fn number(p: &Value, name: &str) -> Result<u64> {
    p.get(name)
        .and_then(Value::as_u64)
        .ok_or_else(|| error("invalid_params", &format!("缺少非负整数 {name}")))
}
fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| error("invalid_params", "参数类型无效"))
}
fn event(name: &str, data: Value) -> Message {
    let message: Message = Event::new(name, data).into();
    if serde_json::to_vec(&message).is_ok_and(|v| v.len() <= MAX_LINE_BYTES) {
        message
    } else {
        Event::new("protocol_error", json!({"code": "event_too_large"})).into()
    }
}
fn response(id: &str, result: Result<Value>) -> Message {
    bounded_response(id, result, MAX_LINE_BYTES)
}
fn bounded_response(id: &str, result: Result<Value>, budget: usize) -> Message {
    let message: Message = match result {
        Ok(value) => Response::success(id, value).expect("已验证请求 ID").into(),
        Err(e) => Response::failure(id, e).expect("已验证请求 ID").into(),
    };
    if serde_json::to_vec(&message).is_ok_and(|v| v.len() <= budget) {
        message
    } else {
        Response::failure(
            id,
            error(
                "response_too_large",
                "响应超过帧预算，请提高 max_bytes 或使用分页/分块接口",
            ),
        )
        .expect("已验证请求 ID")
        .into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppKind {
    Drawing,
    Blackboard,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Permissions {
    classroom_safe: bool,
    desktop_capture_allowed: bool,
    agent_allowed: bool,
}
impl Default for Permissions {
    fn default() -> Self {
        Self {
            classroom_safe: true,
            desktop_capture_allowed: false,
            agent_allowed: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowRequest {
    pub request_id: String,
    pub visible: bool,
}

#[derive(Debug)]
struct Job {
    method: &'static str,
    document_id: String,
    page_id: String,
    revision: u64,
    params: Value,
    request_id: Option<String>,
    lease: Option<String>,
    write_back: bool,
    host_job_id: Option<String>,
    download: Option<host::Download>,
}

#[derive(Debug)]
struct CancelledCapture {
    job_id: String,
    request_id: String,
    host_job_id: Option<String>,
    lease: String,
}

pub struct Session {
    pub document: Document,
    pub history: History,
    pub configured: bool,
    pub desired_visible: bool,
    pub closed: bool,
    pub app: AppKind,
    pub resources: ResourceStore,
    permissions: Permissions,
    hide_leases: HashSet<String>,
    has_window: bool,
    actual_visible: bool,
    window_confirmed: bool,
    window_request: Option<WindowRequest>,
    pending_window_response: Option<String>,
    jobs: HashMap<String, Job>,
    // 已发出的截图被取消时，保留隐藏租约直到 Neo 确认停止，避免取消与截图竞态。
    cancelled_captures: HashMap<String, CancelledCapture>,
    connected: bool,
    close_pending: bool,
    owned_windows: HashSet<String>,
    seen_host_jobs: HashSet<String>,
}

impl Session {
    pub fn new(app: AppKind) -> Self {
        let document = Document::new();
        let history = History::new(&document);
        Self {
            document,
            history,
            configured: false,
            desired_visible: true,
            closed: false,
            app,
            resources: ResourceStore::default(),
            permissions: Permissions::default(),
            hide_leases: HashSet::new(),
            has_window: false,
            actual_visible: false,
            window_confirmed: false,
            window_request: None,
            pending_window_response: None,
            jobs: HashMap::new(),
            cancelled_captures: HashMap::new(),
            connected: true,
            close_pending: false,
            owned_windows: HashSet::from(["main".into()]),
            seen_host_jobs: HashSet::new(),
        }
    }

    pub fn effective_visible(&self) -> bool {
        self.configured
            && self.desired_visible
            && self.hide_leases.is_empty()
            && !self.closed
            && !self.close_pending
    }

    pub fn state(&self) -> Value {
        json!({"app": self.app, "document_id": self.document.id,
            "page_id": self.document.current_page().id, "revision": self.document.revision,
            "revision_scope": "document", "dirty": self.history.is_dirty(&self.document),
            "configured": self.configured, "closed": self.closed,
            "connected": self.connected, "close_pending": self.close_pending,
            "desired_visible": self.desired_visible, "effective_visible": self.effective_visible(),
            "visible": self.has_window && self.actual_visible, "has_window": self.has_window,
            "window_status": if !self.has_window { "no_window" } else if !self.window_confirmed {
                "pending" } else if self.actual_visible { "visible" } else { "hidden" },
            "hidden_confirmed": self.has_window && self.window_confirmed && !self.actual_visible,
            "hide_lease_count": self.hide_leases.len(), "permissions": self.permissions,
            "owned_window_count": self.owned_windows.len(),
            "can_undo": self.history.can_undo(), "can_redo": self.history.can_redo(),
            "pending_jobs": self.jobs.len(), "pending_capture_cancellations": self.cancelled_captures.len()})
    }

    pub fn ready(&self) -> Message {
        let mut methods = vec![
            "configure",
            "show",
            "hide",
            "close",
            "get_state",
            "window.suspend",
            "window.resume",
            "objects.list",
            "objects.read",
            "objects.apply",
            "connections.list",
            "connections.connect",
            "connections.disconnect",
            "undo",
            "redo",
            "pages.add",
            "pages.delete",
            "pages.select",
            "pages.list",
            "document.new",
            "document.open",
            "document.save",
            "math.calculate",
            "resources.import_png",
            "resources.begin",
            "resources.chunk",
            "resources.finish",
            "resources.abort",
            "resources.read",
            "resources.release",
            "agent.request",
            "jobs.cancel",
        ];
        let mut host_methods = vec![
            "host.ask_agent",
            "jobs.cancel",
            "resources.read",
            "resources.release",
        ];
        if self.has_window {
            methods.push("capture.request");
            host_methods.push("host.capture_region");
        }
        event(
            "ready",
            json!({"app": self.app, "methods": methods, "host_methods": host_methods,
            "events": ["state_changed", "document_changed", "window.requested", "job.finished", "protocol_error"],
            "headless": !self.has_window, "has_window": self.has_window,
            "max_line_bytes": MAX_LINE_BYTES, "revision_scope": "document",
            "resource_encoding": "u8_array", "resource_persistence": "board-session-package-v1",
            "resource_persistence_versions": ["board-session-package-v1", "board-session-package-v2"],
            "document_file_versions": [1, 2],
            "object_chunk_encoding": "utf8_json_u8_array",
            "object_chunk_bytes": resources::MAX_READ_BYTES,
            "resource_chunk_bytes": resources::MAX_READ_BYTES,
            "resource_max_bytes": resources::MAX_PNG_BYTES,
            "resource_store_bytes": resources::MAX_STORE_BYTES,
            "upload_integrity": "crc32-ieee-u32", "upload_owner": "request_id_prefix",
            "history_persistence": "current_document_only_history_kept_in_memory",
            "host_events": ["job.finished", "jobs.finished"],
            "host_job_ids": "unique_per_session_max_4096",
            "capture_resource_descriptor": ["asset_ref", "total_bytes", "crc32"],
            "extensions_require_host_adapter": true}),
        )
    }

    // GUI 应在发送 ready 前接入，并隐藏全部所属窗口后才确认 visible=false。
    pub fn attach_window(&mut self) -> Vec<Message> {
        if self.has_window {
            return Vec::new();
        }
        self.has_window = true;
        let mut out = Vec::new();
        self.request_window(None, &mut out);
        out
    }

    pub fn pending_window_request(&self) -> Option<&WindowRequest> {
        self.window_request.as_ref()
    }

    fn request_window(&mut self, response_id: Option<String>, out: &mut Vec<Message>) {
        if !self.has_window {
            if self.close_pending {
                self.closed = true;
                self.close_pending = false;
            }
            if let Some(id) = response_id {
                out.push(response(&id, Ok(self.state())));
            }
            return;
        }
        if response_id.is_none()
            && self
                .window_request
                .as_ref()
                .is_some_and(|r| r.visible == self.effective_visible())
        {
            return;
        }
        if let Some(id) = self.pending_window_response.take() {
            out.push(response(
                &id,
                Err(error("window_superseded", "显隐请求被较新的请求替代")),
            ));
        }
        let request = WindowRequest {
            request_id: new_id(),
            visible: self.effective_visible(),
        };
        self.window_confirmed = false;
        self.pending_window_response = response_id;
        out.push(event("window.requested", json!(request)));
        self.window_request = Some(request);
    }

    /// 兼容聚合确认：适配器必须已确认全部所属窗口；不可只报告主窗口。
    pub fn acknowledge_window(&mut self, request_id: &str, visible: bool) -> Vec<Message> {
        let Some(request) = self.window_request.as_ref() else {
            return Vec::new();
        };
        if request.request_id != request_id || request.visible != visible {
            return Vec::new();
        }
        self.window_request = None;
        self.actual_visible = visible;
        self.window_confirmed = true;
        if self.close_pending && !visible {
            self.closed = true;
            self.close_pending = false;
        }
        let mut out = Vec::new();
        if let Some(id) = self.pending_window_response.take() {
            out.push(response(&id, Ok(self.state())));
        }
        self.dispatch_jobs(&mut out);
        out.push(event("state_changed", self.state()));
        out
    }

    pub fn handle(&mut self, request: Request) -> Vec<Message> {
        // 即便由内存调用而非协议读取器进入，也不能绕过帧大小与 ID 校验。
        if let Err(e) = Message::Request(request.clone()).validate() {
            return vec![event("protocol_error", json!({"code": e.code()}))];
        }
        if request.id.len() > 256 {
            return vec![event(
                "protocol_error",
                json!({"code": "invalid_id", "message": "请求 ID 最多 256 字节"}),
            )];
        }
        if serde_json::to_vec(&request).map_or(true, |b| b.len() > MAX_LINE_BYTES) {
            return vec![response(
                &request.id,
                Err(error("line_too_long", "请求或 ID 过长")),
            )];
        }
        let before = self.state();
        let mut out = Vec::new();
        let result = if self.close_pending && request.method != "get_state" {
            Err(error("session_closing", "等待全部窗口关闭确认"))
        } else if self.closed && request.method != "get_state" {
            Err(error("session_closed", "会话已关闭"))
        } else if !request.params.is_object() {
            Err(error("invalid_params", "params 必须是对象"))
        } else if !self.configured
            && !matches!(request.method.as_str(), "configure" | "get_state" | "close")
        {
            Err(error("not_configured", "必须先 configure"))
        } else {
            self.execute(&request, &mut out)
        };
        let budget = if matches!(
            request.method.as_str(),
            "objects.list" | "pages.list" | "connections.list"
        ) {
            request.params["max_bytes"]
                .as_u64()
                .filter(|v| (1024..=MAX_LINE_BYTES as u64).contains(v))
                .unwrap_or(60 * 1024) as usize
        } else {
            MAX_LINE_BYTES
        };
        match result {
            Ok(Some(value)) => out.insert(0, bounded_response(&request.id, Ok(value), budget)),
            Ok(None) => {}
            Err(e) => out.insert(0, bounded_response(&request.id, Err(e), budget)),
        }
        self.dispatch_jobs(&mut out);
        let after = self.state();
        if before["document_id"] != after["document_id"] || before["revision"] != after["revision"]
        {
            out.push(event("document_changed", after.clone()));
        }
        if before != after {
            out.push(event("state_changed", after));
        }
        out
    }

    fn context(&self, p: &Value, revision: bool) -> Result<String> {
        if text(p, "document_id")? != self.document.id {
            return Err(error("document_mismatch", "文档 ID 不匹配"));
        }
        let page = text(p, "page_id")?;
        if !self.document.pages.iter().any(|v| v.id == page) {
            return Err(error("page_not_found", "页面不存在"));
        }
        if revision && number(p, "expected_revision")? != self.document.revision {
            return Err(error("revision_conflict", "文档版本已经变化"));
        }
        Ok(page.to_owned())
    }

    fn guard_discard(&self, p: &Value) -> Result<()> {
        let discard = p
            .get("discard_unsaved")
            .map(|value| decode::<bool>(value.clone()))
            .transpose()?
            .unwrap_or(false);
        if self.history.is_dirty(&self.document) && !discard {
            Err(error(
                "unsaved_changes",
                "有未保存内容；先保存或显式 discard_unsaved=true",
            ))
        } else {
            Ok(())
        }
    }

    fn operations(&self, p: &Value) -> Result<Vec<Operation>> {
        let operations: Vec<Operation> =
            decode(p.get("operations").cloned().unwrap_or(Value::Null))?;
        for op in &operations {
            if let Operation::Add { object } | Operation::Update { object } = op
                && let ObjectKind::Image { asset_ref, .. } = &object.kind
                && self.resources.get(asset_ref).is_none()
            {
                return Err(error("resource_not_found", "图片必须引用已有内存资源"));
            }
        }
        Ok(operations)
    }

    fn execute(&mut self, r: &Request, out: &mut Vec<Message>) -> Result<Option<Value>> {
        let p = &r.params;
        let value = match r.method.as_str() {
            "configure" => {
                let permissions: Permissions = decode(p.clone())?;
                if self.permissions.classroom_safe != permissions.classroom_safe
                    || self.permissions.desktop_capture_allowed
                        != permissions.desktop_capture_allowed
                    || self.permissions.agent_allowed != permissions.agent_allowed
                {
                    self.resources.abort_owner_uploads("neo");
                    self.resources.abort_owner_uploads("runtime");
                }
                self.permissions = permissions;
                self.configured = true;
                let revoked: Vec<_> = self
                    .jobs
                    .iter()
                    .filter(|(_, job)| {
                        (job.method == "host.capture_region"
                            && (permissions.classroom_safe || !permissions.desktop_capture_allowed))
                            || (job.method == "host.ask_agent" && !permissions.agent_allowed)
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in revoked {
                    self.cancel_job(&id, "permission_revoked", out);
                }
                self.request_window(None, out);
                self.state()
            }
            "get_state" => self.state(),
            "show" | "hide" | "close" | "window.suspend" | "window.resume" => {
                match r.method.as_str() {
                    "show" => self.desired_visible = true,
                    "hide" => self.desired_visible = false,
                    "close" => {
                        self.guard_discard(p)?;
                        self.close_pending = true;
                        self.resources.abort_uploads();
                        let jobs: Vec<_> = self.jobs.keys().cloned().collect();
                        for id in jobs {
                            self.cancel_job(&id, "session_closed", out);
                        }
                    }
                    "window.suspend" => {
                        let lease = text(p, "lease_id")?;
                        if lease.len() > 128
                            || lease.starts_with("capture:")
                            || (self.hide_leases.len() >= 128 && !self.hide_leases.contains(lease))
                        {
                            return Err(error("invalid_params", "租约 ID 无效或租约数量已达上限"));
                        }
                        self.hide_leases.insert(lease.to_owned());
                    }
                    "window.resume" => {
                        let lease = text(p, "lease_id")?;
                        if lease.starts_with("capture:") || !self.hide_leases.remove(lease) {
                            return Err(error("lease_not_found", "外部隐藏租约不存在"));
                        }
                    }
                    _ => unreachable!(),
                }
                self.request_window(Some(r.id.clone()), out);
                return Ok(None);
            }
            "connections.list" => {
                let page = self.context(p, false)?;
                let connections: Vec<_> = self
                    .document
                    .connections
                    .iter()
                    .filter(|c| c.page_id == page)
                    .map(|c| json!(c))
                    .collect();
                self.paginate(r, "connections", &connections)?
            }
            "connections.connect" => {
                let page = self.context(p, true)?;
                let connection: Connection =
                    decode(p.get("connection").cloned().unwrap_or(Value::Null))?;
                let revision = self.document.revision;
                self.history
                    .edit(&mut self.document, |d| {
                        d.connect(&page, revision, connection)
                    })
                    .map_err(core_error)?;
                self.state()
            }
            "connections.disconnect" => {
                let page = self.context(p, true)?;
                let id = text(p, "connection_id")?;
                let revision = self.document.revision;
                self.history
                    .edit(&mut self.document, |d| d.disconnect(&page, revision, id))
                    .map_err(core_error)?;
                self.state()
            }
            "objects.list" => self.list_objects(r)?,
            "objects.read" => self.read_object(p)?,
            "objects.apply" => {
                let page = self.context(p, true)?;
                let operations = self.operations(p)?;
                let revision = self.document.revision;
                self.history
                    .apply(&mut self.document, &page, revision, &operations)
                    .map_err(core_error)?;
                self.state()
            }
            "undo" | "redo" => {
                self.context(p, true)?;
                let changed = if r.method == "undo" {
                    self.history.undo(&mut self.document)
                } else {
                    self.history.redo(&mut self.document)
                }
                .map_err(core_error)?;
                json!({"changed": changed, "state": self.state()})
            }
            "pages.add" => {
                self.context(p, true)?;
                let id = self
                    .history
                    .edit(&mut self.document, Document::add_page)
                    .map_err(core_error)?;
                json!({"page_id": id, "state": self.state()})
            }
            "pages.delete" => {
                let page = self.context(p, true)?;
                self.history
                    .edit(&mut self.document, |d| d.delete_page(&page))
                    .map_err(core_error)?;
                self.state()
            }
            "pages.select" => {
                let page = self.context(p, false)?;
                let index = self
                    .document
                    .pages
                    .iter()
                    .position(|v| v.id == page)
                    .expect("已验证页面");
                self.document.set_current_page(index).map_err(core_error)?;
                self.state()
            }
            "pages.list" => {
                if text(p, "document_id")? != self.document.id {
                    return Err(error("document_mismatch", "文档 ID 不匹配"));
                }
                let pages: Vec<_> = self
                    .document
                    .pages
                    .iter()
                    .map(|v| json!({"page_id": v.id, "object_count": v.objects.len()}))
                    .collect();
                self.paginate(r, "pages", &pages)?
            }
            "document.new" => {
                self.guard_discard(p)?;
                self.replace_document(Document::new(), out);
                self.state()
            }
            "document.open" => {
                self.guard_discard(p)?;
                let (document, resources) = package::load(std::path::Path::new(text(p, "path")?))?;
                self.replace_document(document, out);
                self.resources = resources;
                self.state()
            }
            "document.save" => {
                self.save_document(std::path::Path::new(text(p, "path")?))?;
                self.state()
            }
            "math.calculate" => json!({"result": board_math::calculate(text(p, "expression")?)
                .map_err(|e| ProtocolError::new("math_error", e.to_string()))?}),
            "resources.import_png" => {
                let bytes: Vec<u8> = decode(p.get("bytes").cloned().unwrap_or(Value::Null))?;
                let asset_ref = self.resources.import_png(bytes)?;
                let resource = self.resources.get(&asset_ref).expect("刚导入资源");
                json!({"asset_ref": asset_ref, "width": resource.width, "height": resource.height, "mime_type": "image/png"})
            }
            "resources.begin" => {
                let total = usize::try_from(number(p, "total_bytes")?)
                    .map_err(|_| error("invalid_params", "大小溢出"))?;
                let crc = u32::try_from(number(p, "crc32")?)
                    .map_err(|_| error("invalid_params", "CRC32 必须是 u32"))?;
                let owner = r.id.split_once(':').expect("已验证 ID").0;
                let id = self.resources.begin_upload(owner, total, crc)?;
                json!({"upload_id": id, "max_chunk_bytes": resources::MAX_READ_BYTES})
            }
            "resources.chunk" => {
                let owner = r.id.split_once(':').expect("已验证 ID").0;
                let bytes: Vec<u8> = decode(p.get("bytes").cloned().unwrap_or(Value::Null))?;
                let offset = usize::try_from(number(p, "offset")?)
                    .map_err(|_| error("invalid_params", "偏移溢出"))?;
                let next =
                    self.resources
                        .upload_chunk(owner, text(p, "upload_id")?, offset, &bytes)?;
                json!({"next_offset": next})
            }
            "resources.finish" => {
                let owner = r.id.split_once(':').expect("已验证 ID").0;
                let id = self.resources.finish_upload(owner, text(p, "upload_id")?)?;
                let resource = self.resources.get(&id).unwrap();
                json!({"asset_ref": id, "width": resource.width, "height": resource.height, "mime_type": "image/png"})
            }
            "resources.abort" => {
                let owner = r.id.split_once(':').expect("已验证 ID").0;
                self.resources.abort_upload(owner, text(p, "upload_id")?)?;
                json!({"aborted": true})
            }
            "resources.read" => {
                let offset = usize::try_from(number(p, "offset")?)
                    .map_err(|_| error("invalid_params", "偏移过大"))?;
                let length = usize::try_from(number(p, "length")?)
                    .map_err(|_| error("invalid_params", "长度过大"))?;
                self.resources.read(text(p, "asset_ref")?, offset, length)?
            }
            "resources.release" => {
                // 历史记录可能引用已删除对象；保守地拒绝在存在历史时释放。
                let asset_ref = text(p, "asset_ref")?;
                let used = self.document.pages.iter().flat_map(|p| &p.objects).any(|o|
                    matches!(&o.kind, ObjectKind::Image { asset_ref: id, .. } if id == asset_ref));
                let pending = self.jobs.values().any(|j| {
                    j.params["asset_refs"]
                        .as_array()
                        .is_some_and(|refs| refs.iter().any(|v| v.as_str() == Some(asset_ref)))
                });
                if used || pending || self.history.can_undo() || self.history.can_redo() {
                    return Err(error(
                        "resource_in_use",
                        "文档、历史记录或任务可能仍引用资源",
                    ));
                }
                json!({"released": self.resources.release(asset_ref)})
            }
            "capture.request" | "agent.request" => self.start_job(r, out)?,
            "jobs.cancel" => {
                let id = text(p, "job_id")?;
                if !self.jobs.contains_key(id) {
                    return Err(error("job_not_found", "任务不存在或已结束"));
                }
                self.cancel_job(id, "cancelled", out);
                json!({"cancelled": true, "job_id": id})
            }
            _ => return Err(error("method_not_found", "方法未实现")),
        };
        Ok(Some(value))
    }

    fn replace_document(&mut self, document: Document, out: &mut Vec<Message>) {
        let jobs: Vec<_> = self.jobs.keys().cloned().collect();
        for id in jobs {
            self.cancel_job(&id, "document_replaced", out);
        }
        self.history = History::new(&document);
        self.document = document;
        self.resources = ResourceStore::default();
    }

    // 大对象按 UTF-8 JSON 字节分块；调用方拼接全部字节后再解析，不能逐块解码文本。
    fn read_object(&self, p: &Value) -> Result<Value> {
        let page = self.context(p, true)?;
        let id = text(p, "object_id")?;
        let object = self
            .document
            .pages
            .iter()
            .find(|v| v.id == page)
            .expect("已验证页面")
            .objects
            .iter()
            .find(|o| o.id == id)
            .ok_or_else(|| error("object_not_found", "对象不存在"))?;
        let bytes = serde_json::to_vec(object).expect("已验证对象");
        let offset = usize::try_from(number(p, "offset")?)
            .map_err(|_| error("invalid_params", "偏移过大"))?;
        let length = usize::try_from(number(p, "length")?)
            .map_err(|_| error("invalid_params", "长度过大"))?;
        if offset > bytes.len() || length == 0 || length > resources::MAX_READ_BYTES {
            return Err(error("invalid_params", "读取偏移无效或长度不在 1..8192 内"));
        }
        let end = offset + length.min(bytes.len() - offset);
        Ok(
            json!({"document_id": self.document.id, "page_id": page, "object_id": id,
            "revision": self.document.revision, "encoding": "utf8_json_u8_array",
            "offset": offset, "total_bytes": bytes.len(), "bytes": &bytes[offset..end],
            "next_offset": end, "eof": end == bytes.len()}),
        )
    }

    fn list_objects(&self, r: &Request) -> Result<Value> {
        let page = self.context(&r.params, false)?;
        let objects: Vec<_> = self
            .document
            .pages
            .iter()
            .find(|p| p.id == page)
            .expect("已验证页面")
            .objects
            .iter()
            .map(|o| json!(o))
            .collect();
        self.paginate(r, "objects", &objects)
    }

    fn paginate(&self, r: &Request, key: &str, items: &[Value]) -> Result<Value> {
        let p = &r.params;
        let offset = p
            .get("offset")
            .map(|_| number(p, "offset"))
            .transpose()?
            .unwrap_or(0);
        let offset = usize::try_from(offset).map_err(|_| error("invalid_params", "偏移过大"))?;
        if offset > items.len() {
            return Err(error("invalid_params", "偏移超出列表长度"));
        }
        if (offset > 0 || p.get("expected_revision").is_some())
            && number(p, "expected_revision")? != self.document.revision
        {
            return Err(error("revision_conflict", "分页期间文档已经变化"));
        }
        let budget = p
            .get("max_bytes")
            .map(|_| number(p, "max_bytes"))
            .transpose()?
            .unwrap_or(60 * 1024);
        if !(1024..=MAX_LINE_BYTES as u64).contains(&budget) {
            return Err(error("invalid_params", "max_bytes 必须在 1024..65536 内"));
        }
        let limit = p
            .get("limit")
            .map(|_| number(p, "limit"))
            .transpose()?
            .unwrap_or(100)
            .min(1000) as usize;
        if limit == 0 {
            return Err(error("invalid_params", "limit 必须大于零"));
        }
        let mut result = json!({"document_id": self.document.id, "page_id": p.get("page_id"),
            "revision": self.document.revision, "offset": offset, "total": items.len(),
            "next_offset": null, (key): []});
        let mut selected = Vec::new();
        for item in items.iter().skip(offset).take(limit) {
            selected.push(item.clone());
            let end = offset + selected.len();
            result[key] = json!(selected);
            result["next_offset"] = if end < items.len() {
                json!(end)
            } else {
                Value::Null
            };
            let frame = Response::success(&r.id, result.clone()).expect("已验证 ID");
            if serde_json::to_vec(&frame).expect("JSON 值可序列化").len() > budget as usize {
                selected.pop();
                if selected.is_empty() {
                    let (code, message, id_key) = match key {
                        "connections" => (
                            "connection_too_large",
                            "单个连接超过本页预算；请提高 max_bytes",
                            "connection_id",
                        ),
                        _ => (
                            "object_too_large",
                            "单个对象超过本页预算；可提高 max_bytes 或使用 objects.read 分块读取",
                            "object_id",
                        ),
                    };
                    let mut e = error(code, message);
                    e.data = Some(json!({"document_id": self.document.id,
                        "page_id": p.get("page_id"), "revision": self.document.revision,
                        (id_key): item.get("id").or_else(|| item.get("page_id")), "offset": offset,
                        "total_bytes": serde_json::to_vec(item).expect("JSON 值").len(),
                        "next_offset": if offset + 1 < items.len() { Some(offset + 1) } else { None }}));
                    return Err(e);
                }
                result[key] = json!(selected);
                result["next_offset"] = json!(offset + selected.len());
                break;
            }
        }
        Ok(result)
    }

    fn start_job(&mut self, r: &Request, out: &mut Vec<Message>) -> Result<Value> {
        let p = &r.params;
        if !self.connected {
            return Err(error("host_disconnected", "主机连接已断开"));
        }
        let page = self.context(p, true)?;
        if p.get("user_authorized") != Some(&Value::Bool(true)) {
            return Err(error("authorization_required", "每次请求均需用户显式授权"));
        }
        if self.jobs.len() + self.cancelled_captures.len() >= 32 {
            return Err(error("job_limit", "待完成任务已达上限"));
        }
        let capture = r.method == "capture.request";
        let mut params = json!({"document_id": self.document.id, "page_id": page,
            "revision": self.document.revision, "user_authorized": true});
        let write_back;
        if capture {
            if self.permissions.classroom_safe || !self.permissions.desktop_capture_allowed {
                return Err(error("permission_denied", "安全模式或权限禁止截图"));
            }
            if !self.has_window {
                return Err(error(
                    "window_unavailable",
                    "无窗口模式不能确认 GUI 隐藏，不支持截图",
                ));
            }
            write_back = false;
        } else {
            if !self.permissions.agent_allowed {
                return Err(error("permission_denied", "未允许 Agent"));
            }
            params["prompt"] = json!(text(p, "prompt")?);
            let refs: Vec<String> = decode(p.get("asset_refs").cloned().unwrap_or(json!([])))?;
            if refs.iter().any(|id| self.resources.get(id).is_none()) {
                return Err(error(
                    "resource_not_found",
                    "Agent 附件必须是已存在的资源引用",
                ));
            }
            params["asset_refs"] = json!(refs);
            write_back = p
                .get("write_back")
                .map(|v| decode(v.clone()))
                .transpose()?
                .unwrap_or(false);
            params["write_back"] = json!(write_back);
        }
        let id = format!("job:{}", new_id());
        params["job_id"] = json!(id);
        let lease = if capture {
            Some(format!("capture:{}", new_id()))
        } else {
            None
        };
        if let Some(lease) = &lease {
            self.hide_leases.insert(lease.clone());
        }
        let job = Job {
            method: if capture {
                "host.capture_region"
            } else {
                "host.ask_agent"
            },
            document_id: self.document.id.clone(),
            page_id: page,
            revision: self.document.revision,
            params,
            request_id: None,
            lease,
            write_back,
            host_job_id: None,
            download: None,
        };
        // 先检查最终出站帧，不能返回 job_id 后才发现请求不可传输。
        let preview = Request::new(
            format!("runtime:{}", new_id()),
            job.method,
            job.params.clone(),
        )
        .expect("内部 ID");
        if serde_json::to_vec(&preview).expect("JSON 值").len() > MAX_LINE_BYTES - 512 {
            if let Some(lease) = &job.lease {
                self.hide_leases.remove(lease);
            }
            return Err(error("line_too_long", "主机请求超过大小限制"));
        }
        self.jobs.insert(id.clone(), job);
        if capture {
            self.request_window(None, out);
        }
        Ok(json!({"job_id": id, "status": "pending"}))
    }

    fn dispatch_jobs(&mut self, out: &mut Vec<Message>) {
        let hidden = self.has_window && self.window_confirmed && !self.actual_visible;
        for job in self.jobs.values_mut() {
            if job.request_id.is_some() || (job.lease.is_some() && !hidden) {
                continue;
            }
            let id = format!("runtime:{}", new_id());
            if job.lease.is_some() {
                job.params["windows_hidden_confirmed"] = json!(true);
            }
            out.push(
                Request::new(&id, job.method, job.params.clone())
                    .expect("内部 ID")
                    .into(),
            );
            job.request_id = Some(id);
        }
    }

    fn release_job_lease(&mut self, job: &Job, out: &mut Vec<Message>) {
        if let Some(lease) = &job.lease {
            self.hide_leases.remove(lease);
            self.request_window(None, out);
        }
    }

    fn cancel_job(&mut self, id: &str, reason: &str, out: &mut Vec<Message>) {
        let Some(job) = self.jobs.remove(id) else {
            return;
        };
        let mut wait_for_stop = false;
        if let Some(download) = &job.download {
            let _ = self.resources.abort_upload(id, &download.upload_id);
            self.release_host_resource(&download.asset_ref, out);
        } else if let Some(request_id) = &job.request_id {
            let cancel_id = format!("runtime:{}", new_id());
            if let Some(lease) = &job.lease {
                self.cancelled_captures.insert(
                    cancel_id.clone(),
                    CancelledCapture {
                        job_id: id.to_owned(),
                        request_id: request_id.clone(),
                        host_job_id: job.host_job_id.clone(),
                        lease: lease.clone(),
                    },
                );
                wait_for_stop = true;
            }
            out.push(
                Request::new(
                    cancel_id,
                    "jobs.cancel",
                    json!({"job_id": job.host_job_id.as_deref().unwrap_or(id), "request_id": request_id}),
                )
                .expect("内部 ID")
                .into(),
            );
        }
        if !wait_for_stop {
            self.release_job_lease(&job, out);
        }
        out.push(event("job.finished", json!({"job_id": id, "ok": false, "error": error(reason, "任务已终止；后续结果将忽略")})));
    }

    // Neo 必须对出站 runtime: 请求给出最终响应；并非接受任意文档写回的入口。
    pub fn handle_response(&mut self, incoming: Response) -> Vec<Message> {
        if !self.connected || Message::Response(incoming.clone()).validate().is_err() {
            return Vec::new();
        }
        if serde_json::to_vec(&incoming).map_or(true, |v| v.len() > MAX_LINE_BYTES) {
            let id = self
                .jobs
                .iter()
                .find(|(_, job)| {
                    job.request_id.as_deref() == Some(&incoming.id)
                        || job
                            .download
                            .as_ref()
                            .is_some_and(|d| d.request_id == incoming.id)
                })
                .map(|(id, _)| id.clone());
            let mut out = Vec::new();
            if let Some(id) = id {
                // 无法信任超长帧中的完成标志；截图仍须等待独立停止确认。
                self.cancel_job(&id, "response_too_large", &mut out);
                out.push(event("state_changed", self.state()));
            }
            return out;
        }
        self.process_host_response(incoming)
    }

    fn complete_job(&mut self, job: &Job, result: Value) -> Result<Value> {
        if self.closed
            || self.document.id != job.document_id
            || self.document.revision != job.revision
            || !self.document.pages.iter().any(|p| p.id == job.page_id)
        {
            return Err(error(
                "revision_conflict",
                "异步任务对应的文档、页面或版本已经变化",
            ));
        }
        if job.method == "host.capture_region" {
            if self.permissions.classroom_safe || !self.permissions.desktop_capture_allowed {
                return Err(error("permission_denied", "截图权限已撤销"));
            }
            let asset_ref = if let Some(id) = result.get("asset_ref").and_then(Value::as_str) {
                if self.resources.get(id).is_none() {
                    return Err(error(
                        "resource_not_found",
                        "主机返回的资源必须已导入当前会话",
                    ));
                }
                id.to_owned()
            } else {
                let bytes: Vec<u8> =
                    decode(result.get("png_bytes").cloned().unwrap_or(Value::Null))?;
                self.resources.import_png(bytes)?
            };
            let resource = self.resources.get(&asset_ref).expect("已导入资源");
            Ok(
                json!({"asset_ref": asset_ref, "mime_type": "image/png", "width": resource.width, "height": resource.height}),
            )
        } else {
            if !self.permissions.agent_allowed {
                return Err(error("permission_denied", "Agent 权限已撤销"));
            }
            let answer = result.get("answer").and_then(Value::as_str).unwrap_or("");
            if serde_json::to_vec(&json!({"answer": answer}))
                .map_or(true, |v| v.len() > MAX_LINE_BYTES - 2048)
            {
                return Err(error(
                    "response_too_large",
                    "回答过长；拒绝写回以免部分完成",
                ));
            }
            if result.get("operations").is_some() {
                if !job.write_back {
                    return Err(error("permission_denied", "用户没有授权写回"));
                }
                let operations = self.operations(&result)?;
                self.history
                    .apply(&mut self.document, &job.page_id, job.revision, &operations)
                    .map_err(core_error)?;
            }
            // 不原样转发主机附带的任意字段或潜在路径。
            Ok(json!({"answer": answer, "revision": self.document.revision}))
        }
    }
}

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod tests;
