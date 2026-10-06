//! Executable child-wire fixtures, not a host registry or routing implementation.
//! `neo.runtime.drawing` / `neo.runtime.blackboard` belong to the host registry.
//! Host routing must intersect its whitelist, the direction-specific ready list,
//! and grants; neither a ready capability nor these fixtures authorizes a service.

use board_protocol::{Message, read_message, write_message};
use board_session::{AppKind, Session};
use serde_json::{Value, json};
use std::io::Cursor;

const PERMISSION_FIELDS: [&str; 3] = ["classroom_safe", "desktop_capture_allowed", "agent_allowed"];

struct Fixture {
    session: Session,
    sequence: u64,
}

struct Exchange {
    id: String,
    frames: Vec<Value>,
}

impl Exchange {
    fn response(&self) -> &Value {
        let responses: Vec<_> = self
            .frames
            .iter()
            .filter(|frame| frame["type"] == "response" && frame["id"] == self.id)
            .collect();
        assert_eq!(responses.len(), 1, "{:#?}", self.frames);
        responses[0]
    }

    fn result(&self) -> Value {
        let response = self.response();
        assert_eq!(response["ok"], true, "{response}");
        assert!(response.get("error").is_none());
        response.get("result").expect("success result").clone()
    }

    fn error(&self, code: &str) {
        let response = self.response();
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(response["error"]["code"], code, "{response}");
        assert!(response.get("result").is_none());
        assert_eq!(self.frames.len(), 1, "rejection must have no side effects");
    }

    fn has_response(&self) -> bool {
        self.frames
            .iter()
            .any(|frame| frame["type"] == "response" && frame["id"] == self.id)
    }
}

fn wire(message: &Message) -> Value {
    let mut bytes = Vec::new();
    write_message(&mut bytes, message).unwrap();
    assert_eq!(bytes.last(), Some(&b'\n'));
    let parsed = read_message(&mut Cursor::new(&bytes)).unwrap().unwrap();
    assert_eq!(&parsed, message);
    let frame: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(frame["version"].as_u64(), Some(1));
    for field in ["api_version", "grants", "session_generation"] {
        assert!(frame.get(field).is_none(), "not a child envelope field");
    }
    frame
}

fn frames(messages: Vec<Message>) -> Vec<Value> {
    messages.iter().map(wire).collect()
}

impl Fixture {
    fn new(app: AppKind, attached: bool) -> Self {
        let mut session = Session::new(app);
        if attached {
            frames(session.attach_window());
            frames(session.set_owned_windows(&["main", "tools"]).unwrap());
        }
        Self {
            session,
            sequence: 0,
        }
    }

    fn deliver(&mut self, frame: Value) -> Vec<Value> {
        let mut bytes = serde_json::to_vec(&frame).unwrap();
        bytes.push(b'\n');
        let message = read_message(&mut Cursor::new(bytes)).unwrap().unwrap();
        frames(match message {
            Message::Request(request) => self.session.handle(request),
            Message::Response(response) => self.session.handle_response(response),
            Message::Event(event) => self.session.handle_event(event),
        })
    }

    fn call(&mut self, method: &str, params: Value) -> Exchange {
        self.sequence += 1;
        let id = format!("neo:fixture-{}", self.sequence);
        let frames = self.deliver(json!({
            "version": 1, "type": "request", "id": id,
            "method": method, "params": params
        }));
        Exchange { id, frames }
    }

    fn state(&mut self) -> Value {
        self.call("get_state", json!({})).result()
    }

    fn context(&mut self) -> Value {
        let state = self.state();
        assert!(state["revision"].is_u64());
        json!({"document_id": state["document_id"], "page_id": state["page_id"],
            "expected_revision": state["revision"]})
    }

    fn acknowledge(&mut self) -> Vec<Value> {
        let request = self.session.pending_window_request().unwrap().clone();
        frames(self.session.acknowledge_windows(
            &request.request_id,
            &[("main", request.visible), ("tools", request.visible)],
        ))
    }

    fn configure(&mut self, permissions: Value) -> Value {
        let state = self.call("configure", permissions.clone()).result();
        assert_eq!(state["configured"], true);
        assert_eq!(state["permissions"], permissions);
        if state["has_window"] == true {
            self.acknowledge();
        }
        state
    }

    fn reject_unchanged(&mut self, method: &str, params: Value, code: &str) {
        let before = self.state();
        let window = self.session.pending_window_request().cloned();
        self.call(method, params).error(code);
        assert_eq!(self.state(), before);
        assert_eq!(self.session.pending_window_request(), window.as_ref());
    }
}

fn permissions(safe: bool, capture: bool, agent: bool) -> Value {
    json!({"classroom_safe": safe, "desktop_capture_allowed": capture, "agent_allowed": agent})
}

fn normalized_state(mut state: Value) -> Value {
    for (field, placeholder) in [("document_id", "<document>"), ("page_id", "<page>")] {
        assert!(!state[field].as_str().unwrap().is_empty());
        state[field] = json!(placeholder);
    }
    state
}

fn names(value: &Value) -> Vec<&str> {
    let mut names: Vec<_> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    names.sort_unstable();
    let mut unique = names.clone();
    unique.dedup();
    assert_eq!(names, unique, "duplicate advertisement");
    names
}

#[test]
fn ready_and_initial_state_are_directional_child_wire_fixtures() {
    for app in [AppKind::Drawing, AppKind::Blackboard] {
        for attached in [false, true] {
            let mut fixture = Fixture::new(app, attached);
            let ready = wire(&fixture.session.ready());
            assert_eq!(ready["type"], "event");
            assert_eq!(ready["event"], "ready");
            let data = &ready["data"];
            assert_eq!(data["app"], json!(app));
            assert_eq!(data["headless"], !attached);
            assert_eq!(data["has_window"], attached);
            assert_eq!(data["max_line_bytes"], 65536);
            assert_eq!(data["revision_scope"], "document");
            assert_eq!(data["document_file_versions"], json!([1, 2, 3]));
            assert_eq!(
                data["resource_persistence_versions"],
                json!([
                    "board-session-package-v1",
                    "board-session-package-v2",
                    "board-session-package-v3"
                ])
            );
            assert!(names(&data["object_types"]).contains(&"handwritten"));
            for field in [
                "document_id",
                "page_id",
                "revision",
                "api_version",
                "grants",
                "session_generation",
                "capabilities",
            ] {
                assert!(data.get(field).is_none(), "unexpected ready field {field}");
            }
            for registry_id in ["neo.runtime.drawing", "neo.runtime.blackboard"] {
                assert!(!ready.to_string().contains(registry_id));
            }
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
            if attached {
                methods.push("capture.request");
                host_methods.push("host.capture_region");
            }
            methods.sort_unstable();
            host_methods.sort_unstable();
            assert_eq!(names(&data["methods"]), methods);
            assert_eq!(names(&data["host_methods"]), host_methods);
            assert_eq!(
                names(&data["events"]),
                vec![
                    "document_changed",
                    "job.finished",
                    "protocol_error",
                    "state_changed",
                    "window.requested"
                ]
            );
            let state = fixture.state();
            assert!(state["revision"].is_u64());
            assert_eq!(
                normalized_state(state),
                json!({
                    "app": app, "document_id": "<document>", "page_id": "<page>",
                    "revision": 0, "revision_scope": "document", "dirty": false,
                    "configured": false, "closed": false, "connected": true, "close_pending": false,
                    "desired_visible": true, "effective_visible": false, "visible": false,
                    "has_window": attached, "window_status": if attached { "pending" } else { "no_window" },
                    "hidden_confirmed": false, "hide_lease_count": 0,
                    "permissions": permissions(true, false, false),
                    "owned_window_count": if attached { 2 } else { 1 },
                    "can_undo": false, "can_redo": false, "pending_jobs": 0,
                    "pending_capture_cancellations": 0
                })
            );
        }
    }
}

#[test]
fn configure_requires_exactly_three_booleans_and_rejection_is_atomic() {
    for app in [AppKind::Drawing, AppKind::Blackboard] {
        for attached in [false, true] {
            let mut fixture = Fixture::new(app, attached);
            for configured in [false, true] {
                if configured {
                    fixture.configure(permissions(false, true, true));
                }
                fixture.reject_unchanged("configure", json!({}), "invalid_params");
                for field in PERMISSION_FIELDS {
                    let mut missing = permissions(true, false, false);
                    missing.as_object_mut().unwrap().remove(field);
                    fixture.reject_unchanged("configure", missing, "invalid_params");
                    for invalid in [Value::Null, json!(0), json!("true"), json!([]), json!({})] {
                        let mut params = permissions(true, false, false);
                        params[field] = invalid;
                        fixture.reject_unchanged("configure", params, "invalid_params");
                    }
                }
                for (field, value) in [
                    ("api_version", json!(1)),
                    ("grants", json!(["agent.request"])),
                    ("session_generation", json!(1)),
                ] {
                    let mut params = permissions(true, false, false);
                    params[field] = value;
                    fixture.reject_unchanged("configure", params, "invalid_params");
                }
            }
        }
    }
}

#[test]
fn preconfigure_gate_and_clean_close_wait_for_attached_window_ack() {
    for app in [AppKind::Drawing, AppKind::Blackboard] {
        for attached in [false, true] {
            let mut fixture = Fixture::new(app, attached);
            let ready = wire(&fixture.session.ready());
            for method in names(&ready["data"]["methods"]) {
                if !["configure", "get_state", "close"].contains(&method) {
                    fixture.reject_unchanged(method, json!({}), "not_configured");
                }
            }
            let mut close = fixture.call("close", json!({}));
            if attached {
                // Raw Session cannot know whether the adapter created a native window.
                assert!(!close.has_response());
                let pending = fixture.state();
                assert_eq!(pending["configured"], false);
                assert_eq!(pending["closed"], false);
                assert_eq!(pending["close_pending"], true);
                assert_eq!(pending["effective_visible"], false);
                assert_eq!(pending["window_status"], "pending");
                assert_eq!(pending["hidden_confirmed"], false);
                let request = fixture.session.pending_window_request().unwrap().clone();
                assert!(!request.visible);
                let event = close
                    .frames
                    .iter()
                    .find(|frame| frame["event"] == "window.requested")
                    .unwrap();
                assert_eq!(
                    event["data"],
                    json!({"request_id": request.request_id, "visible": false})
                );
                close.frames.extend(fixture.acknowledge());
            }
            let closed = close.result();
            assert_eq!(closed["configured"], false);
            assert_eq!(closed["closed"], true);
            assert_eq!(closed["close_pending"], false);
            assert_eq!(closed["visible"], false);
            assert_eq!(closed["hidden_confirmed"], attached);
            assert!(fixture.session.pending_window_request().is_none());
            assert_eq!(fixture.state(), closed);
        }
    }
}

#[test]
fn advertised_capabilities_never_replace_permissions_or_per_call_authorization() {
    for app in [AppKind::Drawing, AppKind::Blackboard] {
        for attached in [false, true] {
            for safe in [false, true] {
                for capture_allowed in [false, true] {
                    for agent_allowed in [false, true] {
                        let mut fixture = Fixture::new(app, attached);
                        let ready = wire(&fixture.session.ready());
                        fixture.configure(permissions(safe, capture_allowed, agent_allowed));
                        assert_eq!(wire(&fixture.session.ready()), ready);
                        for method in ["agent.request", "capture.request"] {
                            let mut params = fixture.context();
                            if method == "agent.request" {
                                params["prompt"] = json!("fixture only; no real service");
                            }
                            fixture.reject_unchanged(
                                method,
                                params.clone(),
                                "authorization_required",
                            );
                            for invalid in [json!(false), json!("true"), json!(1), Value::Null] {
                                params["user_authorized"] = invalid;
                                fixture.reject_unchanged(
                                    method,
                                    params.clone(),
                                    "authorization_required",
                                );
                            }
                            params["user_authorized"] = json!(true);
                            let denial = if method == "agent.request" {
                                (!agent_allowed).then_some("permission_denied")
                            } else if safe || !capture_allowed {
                                Some("permission_denied")
                            } else if !attached {
                                Some("window_unavailable")
                            } else {
                                None
                            };
                            if let Some(code) = denial {
                                fixture.reject_unchanged(method, params, code);
                                continue;
                            }
                            let exchange = fixture.call(method, params.clone());
                            let pending = exchange.result();
                            assert_eq!(pending["status"], "pending");
                            let mut output = exchange.frames;
                            let host_method = if method == "capture.request" {
                                assert!(!output.iter().any(|f| f["type"] == "request"));
                                assert!(!fixture.session.pending_window_request().unwrap().visible);
                                output.extend(fixture.acknowledge());
                                "host.capture_region"
                            } else {
                                "host.ask_agent"
                            };
                            let requests: Vec<_> =
                                output.iter().filter(|f| f["type"] == "request").collect();
                            assert_eq!(requests.len(), 1);
                            let request = requests[0];
                            assert!(request["id"].as_str().unwrap().starts_with("runtime:"));
                            let mut normalized = request.clone();
                            normalized["id"] = json!("<runtime-request>");
                            assert_eq!(request["params"]["job_id"], pending["job_id"]);
                            normalized["params"]["job_id"] = json!("<job>");
                            let mut expected = json!({
                                "document_id": params["document_id"], "page_id": params["page_id"],
                                "revision": params["expected_revision"], "user_authorized": true, "job_id": "<job>"
                            });
                            if method == "capture.request" {
                                expected["windows_hidden_confirmed"] = json!(true);
                            } else {
                                expected["prompt"] = params["prompt"].clone();
                                expected["asset_refs"] = json!([]);
                                expected["write_back"] = json!(false);
                            }
                            assert_eq!(
                                normalized,
                                json!({
                                    "version": 1, "type": "request", "id": "<runtime-request>",
                                    "method": host_method, "params": expected
                                })
                            );
                            let finished = fixture.deliver(json!({
                                "version": 1, "type": "response", "id": request["id"], "ok": false,
                                "error": {"code": "fixture_declined", "message": "simulated host only"}
                            }));
                            let event = finished
                                .iter()
                                .find(|f| f["event"] == "job.finished")
                                .unwrap();
                            assert_eq!(
                                event["data"],
                                json!({
                                    "job_id": pending["job_id"], "ok": false,
                                    "error": {"code": "fixture_declined", "message": "simulated host only"}
                                })
                            );
                            if fixture.session.pending_window_request().is_some() {
                                fixture.acknowledge();
                            }
                            assert_eq!(fixture.state()["pending_jobs"], 0);
                        }
                        for method in ["host.ask_agent", "host.capture_region", "window.ack"] {
                            fixture.reject_unchanged(method, json!({}), "method_not_found");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn headless_show_never_claims_a_visible_or_confirmed_hidden_window() {
    for app in [AppKind::Drawing, AppKind::Blackboard] {
        let mut fixture = Fixture::new(app, false);
        fixture.configure(permissions(true, false, false));
        for method in ["hide", "show"] {
            let exchange = fixture.call(method, json!({}));
            let state = exchange.result();
            assert_eq!(state["desired_visible"], method == "show");
            assert_eq!(state["visible"], false);
            assert_eq!(state["hidden_confirmed"], false);
            assert_eq!(state["window_status"], "no_window");
            assert!(
                !exchange
                    .frames
                    .iter()
                    .any(|f| f["event"] == "window.requested")
            );
        }
    }
}

#[test]
fn dirty_close_requires_explicit_discard_and_configured_windows_wait_for_all_acks() {
    for app in [AppKind::Drawing, AppKind::Blackboard] {
        for attached in [false, true] {
            let mut fixture = Fixture::new(app, attached);
            fixture.configure(permissions(true, false, false));
            let params = fixture.context();
            fixture.call("pages.add", params).result();
            let before = fixture.state();
            assert_eq!(before["dirty"], true);
            for params in [json!({}), json!({"discard_unsaved": false})] {
                fixture.reject_unchanged("close", params, "unsaved_changes");
            }
            for invalid in [json!("true"), json!(1), Value::Null] {
                fixture.reject_unchanged(
                    "close",
                    json!({"discard_unsaved": invalid}),
                    "invalid_params",
                );
            }
            let mut close = fixture.call("close", json!({"discard_unsaved": true}));
            if attached {
                assert!(!close.has_response());
                let pending = fixture.state();
                assert_eq!(pending["closed"], false);
                assert_eq!(pending["close_pending"], true);
                assert_eq!(pending["effective_visible"], false);
                assert_eq!(pending["visible"], true);
                let request = fixture.session.pending_window_request().unwrap().clone();
                assert!(!request.visible);
                let event = close
                    .frames
                    .iter()
                    .find(|f| f["event"] == "window.requested")
                    .unwrap();
                assert_eq!(
                    event["data"],
                    json!({"request_id": request.request_id, "visible": false})
                );
                for observations in [
                    vec![("main", false)],
                    vec![("main", false), ("tools", true)],
                ] {
                    assert!(
                        fixture
                            .session
                            .acknowledge_windows(&request.request_id, &observations)
                            .is_empty()
                    );
                    assert_eq!(fixture.state(), pending);
                }
                let stale_id = format!("{}-stale", request.request_id);
                assert!(
                    fixture
                        .session
                        .acknowledge_window(&stale_id, false)
                        .is_empty()
                );
                fixture.reject_unchanged("show", json!({}), "session_closing");
                close.frames.extend(fixture.acknowledge());
            }
            let closed = close.result();
            assert_eq!(closed["closed"], true);
            assert_eq!(closed["close_pending"], false);
            assert_eq!(closed["visible"], false);
            assert_eq!(closed["hidden_confirmed"], attached);
            assert_eq!(closed["document_id"], before["document_id"]);
            assert_eq!(closed["page_id"], before["page_id"]);
            assert_eq!(closed["revision"], before["revision"]);
            assert_eq!(fixture.state(), closed);
            fixture.reject_unchanged("show", json!({}), "session_closed");
        }
    }
}

struct SavedFixture(std::path::PathBuf);

impl Drop for SavedFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn file_v3_does_not_change_wire_v1_or_integer_document_revision() {
    for app in [AppKind::Drawing, AppKind::Blackboard] {
        for attached in [false, true] {
            let mut fixture = Fixture::new(app, attached);
            fixture.configure(permissions(true, false, false));
            let mut params = fixture.context();
            let revision = params["expected_revision"].as_u64().unwrap();
            params["operations"] = json!([{"op": "add", "object": {
                "id": board_core::new_id(), "kind": {
                    "type": "handwritten", "position": {"x": 0, "y": 0}, "text": "1",
                    "strokes": [{"points": [{"x": 0, "y": 0, "time": 0, "pressure": 1}],
                        "style": {"color": {"r": 0, "g": 0, "b": 0, "a": 255}, "width": 2, "dashed": false}}]
                }
            }}]);
            let changed = fixture.call("objects.apply", params).result();
            assert_eq!(changed["revision"].as_u64(), Some(revision + 1));
            let saved = SavedFixture(std::env::temp_dir().join(format!(
                "runtime-adapter-handshake-{}.neoboard",
                board_core::new_id()
            )));
            let state = fixture
                .call("document.save", json!({"path": saved.0}))
                .result();
            let file: Value = serde_json::from_slice(&std::fs::read(&saved.0).unwrap()).unwrap();
            assert_eq!(file["version"].as_u64(), Some(3));
            assert_eq!(file["document"]["revision"], changed["revision"]);
            assert_eq!(state["revision"], changed["revision"]);
            assert_eq!(state["dirty"], false);
            assert_eq!(wire(&fixture.session.ready())["version"].as_u64(), Some(1));
            for invalid in [json!("1"), json!(1.5), json!(-1)] {
                let mut params = fixture.context();
                params["expected_revision"] = invalid;
                fixture.reject_unchanged("pages.add", params, "invalid_params");
            }
        }
    }
}
