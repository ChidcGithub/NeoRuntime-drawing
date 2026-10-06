//! Runtime adapter lifecycle contract, using only public Session entry points.
//!
//! Window observations and host replies are simulated: no GUI, desktop capture,
//! host service, or child process is started. Handshake and resource chunking are
//! deliberately outside this target (see api/PROTOCOL.md and api/DRAWING_API.md).
//!
//! Scope limit: cross-process/generation fencing belongs to the host/mainMod
//! adapter. Session has no child-generation or reconnect API; document.new is
//! document replacement, NOT a generation reset. These tests do not claim to
//! validate host fencing. There is no automatic replay: releasing a cancellation
//! slot, replacing a document, or confirming a disconnected host stopped does
//! not resubmit work. A host must explicitly decide whether to issue a new,
//! independently authorized request after reconnecting to a new child.

use board_protocol::{Event, Message, Request, Response, write_message};
use board_session::{AppKind, Session};
use serde_json::{Value, json};

struct Adapter {
    session: Session,
    transcript: Vec<Message>,
    next_id: usize,
}

impl Adapter {
    fn new(app: AppKind) -> Self {
        let mut adapter = Self {
            session: Session::new(app),
            transcript: Vec::new(),
            next_id: 0,
        };
        success(&adapter.call("configure", permissions(true)));
        adapter
    }

    fn record(&mut self, out: Vec<Message>) -> Vec<Message> {
        for message in &out {
            write_message(&mut Vec::new(), message).unwrap();
        }
        self.transcript.extend(out.iter().cloned());
        out
    }

    fn send(&mut self, id: &str, method: &str, params: Value) -> Vec<Message> {
        let out = self
            .session
            .handle(Request::new(id, method, params).unwrap());
        self.record(out)
    }

    fn call(&mut self, method: &str, params: Value) -> Vec<Message> {
        self.next_id += 1;
        self.send(&format!("neo:lifecycle-{}", self.next_id), method, params)
    }

    fn reply(&mut self, id: &str, result: Value) -> Vec<Message> {
        let out = self
            .session
            .handle_response(Response::success(id, result).unwrap());
        self.record(out)
    }

    fn finish(&mut self, job: &str, request: &str, result: Value) -> Vec<Message> {
        let out = self.session.handle_event(Event::new(
            "job.finished",
            json!({"job_id": job, "request_id": request, "ok": true, "result": result}),
        ));
        self.record(out)
    }

    fn observe(&mut self, id: &str, windows: &[(&str, bool)]) -> Vec<Message> {
        let out = self.session.acknowledge_windows(id, windows);
        self.record(out)
    }

    fn ack_all(&mut self) -> Vec<Message> {
        let request = self.session.pending_window_request().unwrap().clone();
        self.observe(
            &request.request_id,
            &[("main", request.visible), ("tools", request.visible)],
        )
    }

    fn windows(&mut self) {
        let out = self.session.set_owned_windows(&["main", "tools"]).unwrap();
        self.record(out);
        self.ack_all();
        assert_eq!(self.session.state()["visible"], true);
    }

    fn disconnect(&mut self) -> Vec<Message> {
        // This is the public hook used by the adapter on EOF, not proof of stop.
        let out = self.session.host_disconnected();
        self.record(out)
    }

    fn context(&self) -> Value {
        let state = self.session.state();
        json!({"document_id": state["document_id"], "page_id": state["page_id"],
            "expected_revision": state["revision"]})
    }

    fn job_params(&self) -> Value {
        let mut params = self.context();
        params["user_authorized"] = json!(true);
        params["prompt"] = json!("An explicitly authorized test request");
        params["write_back"] = json!(true);
        params
    }

    fn agent(&mut self) -> (String, Request) {
        let out = self.call("agent.request", self.job_params());
        (pending_job(&out), outbound(&out, "host.ask_agent"))
    }

    fn capture(&mut self) -> (String, Request) {
        let mut params = self.context();
        params["user_authorized"] = json!(true);
        let out = self.call("capture.request", params);
        let job = pending_job(&out);
        assert!(!out.iter().any(|m| matches!(m, Message::Request(_))));
        assert!(!self.session.pending_window_request().unwrap().visible);
        let request = outbound(&self.ack_all(), "host.capture_region");
        assert_eq!(request.params["windows_hidden_confirmed"], true);
        (job, request)
    }

    fn dirty(&mut self) {
        let mut params = self.context();
        params["operations"] = operations("local-content");
        success(&self.call("objects.apply", params));
        assert_eq!(self.session.state()["dirty"], true);
    }

    fn terminal(&self, job: &str, code: &str) {
        let terminals: Vec<_> = self
            .transcript
            .iter()
            .filter_map(|m| match m {
                Message::Event(e) if e.event == "job.finished" && e.data["job_id"] == job => {
                    Some(&e.data)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            terminals.len(),
            1,
            "terminal count for {job}: {terminals:?}"
        );
        assert_eq!(terminals[0]["ok"], false);
        assert_eq!(terminals[0]["error"]["code"], code);
        assert!(terminals[0].get("result").is_none());
        assert!(terminals[0].get("status").is_none());
    }

    fn request_count(&self, method: &str) -> usize {
        self.transcript
            .iter()
            .filter(|m| matches!(m, Message::Request(r) if r.method == method))
            .count()
    }
}

fn permissions(agent_allowed: bool) -> Value {
    json!({"classroom_safe": false, "desktop_capture_allowed": true, "agent_allowed": agent_allowed})
}

fn response(out: &[Message]) -> &Response {
    let responses: Vec<_> = out
        .iter()
        .filter_map(|m| match m {
            Message::Response(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(responses.len(), 1, "expected one response: {out:?}");
    responses[0]
}

fn success(out: &[Message]) -> Value {
    let response = response(out);
    assert!(response.ok, "{:?}", response.error);
    response.result.clone().unwrap()
}

fn failure(out: &[Message], code: &str) {
    let response = response(out);
    assert!(!response.ok);
    assert_eq!(response.error.as_ref().unwrap().code, code);
    assert!(response.result.is_none());
}

fn pending_job(out: &[Message]) -> String {
    let result = success(out);
    assert_eq!(result["status"], "pending");
    result["job_id"].as_str().unwrap().to_owned()
}

fn outbound(out: &[Message], method: &str) -> Request {
    let requests: Vec<_> = out
        .iter()
        .filter_map(|m| match m {
            Message::Request(r) if r.method == method => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(requests.len(), 1, "expected one {method}: {out:?}");
    assert!(requests[0].id.starts_with("runtime:"));
    requests[0].clone()
}

fn operations(id: &str) -> Value {
    json!([{"op": "add", "object": {"id": id, "kind": {"type": "text",
        "position": {"x": 10, "y": 20}, "text": "Lifecycle write-back", "size": 18,
        "color": {"r": 0, "g": 0, "b": 0, "a": 255}}}}])
}

fn writeback() -> Value {
    json!({"answer": "Done", "operations": operations("host-content")})
}

fn retained_capture(adapter: &Adapter, count: usize) {
    let state = adapter.session.state();
    assert_eq!(state["pending_capture_cancellations"], count);
    assert_eq!(state["hide_lease_count"], count);
    assert_eq!(state["desired_visible"], true);
    assert_eq!(state["effective_visible"], false);
    assert_eq!(state["visible"], false);
    assert!(
        !adapter
            .session
            .pending_window_request()
            .is_some_and(|r| r.visible)
    );
}

#[test]
fn cancel_before_host_ack_rebinds_stop_request_without_duplicate_terminal() {
    for app in [AppKind::Drawing, AppKind::Blackboard] {
        let mut adapter = Adapter::new(app);
        adapter.windows();
        let (job, capture) = adapter.capture();
        let out = adapter.call("jobs.cancel", json!({"job_id": job}));
        assert_eq!(success(&out), json!({"cancelled": true, "job_id": job}));
        let first = outbound(&out, "jobs.cancel");
        assert_eq!(
            first.params,
            json!({"job_id": job, "request_id": capture.id})
        );
        adapter.terminal(&job, "cancelled");
        assert_eq!(adapter.session.state()["pending_jobs"], 0);
        retained_capture(&adapter, 1);

        let second = outbound(
            &adapter.reply(&capture.id, json!({"job_id": "host:late"})),
            "jobs.cancel",
        );
        assert_ne!(first.id, second.id);
        assert_ne!(capture.id, second.id);
        assert_eq!(
            second.params,
            json!({"job_id": "host:late", "request_id": capture.id})
        );
        for (id, result) in [
            (&first.id, json!({"cancelled": true, "job_id": job})),
            (
                &second.id,
                json!({"cancelled": true, "job_id": "host:wrong"}),
            ),
            (
                &second.id,
                json!({"cancelled": false, "job_id": "host:late"}),
            ),
        ] {
            let before = adapter.session.state();
            assert!(adapter.reply(id, result).is_empty());
            assert_eq!(adapter.session.state(), before);
            retained_capture(&adapter, 1);
        }
        let stopped = json!({"cancelled": true, "job_id": "host:late"});
        adapter.reply(&second.id, stopped.clone());
        assert_eq!(adapter.session.state()["pending_capture_cancellations"], 0);
        assert_eq!(adapter.session.state()["hide_lease_count"], 0);
        assert!(adapter.session.effective_visible());
        assert_eq!(adapter.session.state()["visible"], false);
        adapter.ack_all();
        assert_eq!(adapter.session.state()["visible"], true);
        assert!(adapter.reply(&second.id, stopped).is_empty());
        assert!(
            adapter
                .finish("host:late", &capture.id, json!({}))
                .is_empty()
        );
        failure(
            &adapter.call("jobs.cancel", json!({"job_id": job})),
            "job_not_found",
        );
        adapter.terminal(&job, "cancelled");
        assert_eq!(adapter.request_count("host.capture_region"), 1);
        assert_eq!(adapter.request_count("jobs.cancel"), 2);
    }
}

#[test]
fn revoked_agent_permission_blocks_late_writeback_even_after_reauthorization() {
    for app in [AppKind::Drawing, AppKind::Blackboard] {
        for host_ack in [false, true] {
            let mut adapter = Adapter::new(app);
            adapter.dirty();
            let (job, agent) = adapter.agent();
            if host_ack {
                adapter.reply(&agent.id, json!({"job_id": "host:revoked"}));
            }
            let document = adapter.session.document.clone();
            let out = adapter.call("configure", permissions(false));
            success(&out);
            let cancel = outbound(&out, "jobs.cancel");
            assert_eq!(cancel.params["request_id"], agent.id);
            assert_eq!(
                cancel.params["job_id"],
                if host_ack { "host:revoked" } else { &job }
            );
            adapter.terminal(&job, "permission_revoked");
            let before = adapter.session.state();
            let late = if host_ack {
                adapter.finish("host:revoked", &agent.id, writeback())
            } else {
                adapter.reply(&agent.id, writeback())
            };
            assert!(late.is_empty());
            assert_eq!(adapter.session.state(), before);
            assert_eq!(adapter.session.document, document);

            success(&adapter.call("configure", permissions(true)));
            let before = adapter.session.state();
            assert!(adapter.reply(&agent.id, writeback()).is_empty());
            assert!(
                adapter
                    .finish("host:revoked", &agent.id, writeback())
                    .is_empty()
            );
            assert_eq!(adapter.session.state(), before);
            assert_eq!(adapter.session.document, document);
            adapter.terminal(&job, "permission_revoked");
            assert_eq!(adapter.request_count("host.ask_agent"), 1);
            assert_eq!(adapter.session.state()["pending_jobs"], 0);
        }
    }
}

#[test]
fn eof_disconnect_retains_capture_lease_until_independent_stop_proof() {
    for app in [AppKind::Drawing, AppKind::Blackboard] {
        let mut adapter = Adapter::new(app);
        adapter.windows();
        adapter.dirty();
        let (capture_job, capture) = adapter.capture();
        adapter.reply(&capture.id, json!({"job_id": "host:eof-capture"}));
        let (agent_job, agent) = adapter.agent();
        let document = adapter.session.document.clone();
        let out = adapter.disconnect();
        assert!(!out.iter().any(|m| matches!(m, Message::Request(_))));
        adapter.terminal(&capture_job, "host_disconnected");
        adapter.terminal(&agent_job, "host_disconnected");
        assert_eq!(adapter.session.state()["connected"], false);
        assert_eq!(adapter.session.state()["closed"], false);
        assert_eq!(adapter.session.state()["dirty"], true);
        assert_eq!(adapter.session.state()["pending_jobs"], 0);
        retained_capture(&adapter, 1);
        let before = adapter.session.state();
        assert!(adapter.disconnect().is_empty());
        assert!(adapter.reply(&agent.id, writeback()).is_empty());
        assert!(
            adapter
                .finish("host:eof-capture", &capture.id, json!({}))
                .is_empty()
        );
        assert_eq!(adapter.session.state(), before);
        success(&adapter.call("configure", permissions(true)));
        failure(
            &adapter.call("agent.request", adapter.job_params()),
            "host_disconnected",
        );
        retained_capture(&adapter, 1);
        assert_eq!(adapter.session.document, document);

        // Separate simulated adapter evidence that the host stopped, NEVER EOF.
        let out = adapter.session.confirm_host_stopped();
        adapter.record(out);
        assert_eq!(adapter.session.state()["hide_lease_count"], 0);
        assert_eq!(adapter.session.state()["pending_capture_cancellations"], 0);
        adapter.ack_all();
        assert_eq!(adapter.session.state()["visible"], true);
        assert_eq!(adapter.session.state()["connected"], false);
        failure(
            &adapter.call("agent.request", adapter.job_params()),
            "host_disconnected",
        );
        assert_eq!(adapter.session.document, document);
        adapter.terminal(&capture_job, "host_disconnected");
        adapter.terminal(&agent_job, "host_disconnected");
        assert_eq!(adapter.request_count("host.capture_region"), 1);
        assert_eq!(adapter.request_count("host.ask_agent"), 1);
    }
}

#[test]
fn pending_and_cancel_waits_share_32_slots_across_document_replacement() {
    for app in [AppKind::Drawing, AppKind::Blackboard] {
        let mut adapter = Adapter::new(app);
        adapter.windows();
        let mut cancelled = Vec::new();
        for _ in 0..31 {
            let (job, _) = adapter.capture();
            let out = adapter.call("jobs.cancel", json!({"job_id": job}));
            success(&out);
            cancelled.push((job, outbound(&out, "jobs.cancel")));
        }
        let (agent_job, agent) = adapter.agent();
        retained_capture(&adapter, 31);
        assert_eq!(adapter.session.state()["pending_jobs"], 1);
        for method in ["agent.request", "capture.request"] {
            let before = adapter.session.state();
            failure(&adapter.call(method, adapter.job_params()), "job_limit");
            assert_eq!(adapter.session.state(), before);
        }

        let old_document = adapter.session.state()["document_id"].clone();
        success(&adapter.call("document.new", json!({})));
        assert_ne!(adapter.session.state()["document_id"], old_document);
        adapter.terminal(&agent_job, "document_replaced");
        retained_capture(&adapter, 31);
        assert_eq!(adapter.session.state()["pending_jobs"], 0);
        let before = adapter.session.document.clone();
        assert!(adapter.reply(&agent.id, writeback()).is_empty());
        assert_eq!(adapter.session.document, before);
        let (job, _) = adapter.capture();
        let out = adapter.call("jobs.cancel", json!({"job_id": job}));
        cancelled.push((job, outbound(&out, "jobs.cancel")));
        retained_capture(&adapter, 32);
        success(&adapter.call("document.new", json!({})));
        retained_capture(&adapter, 32);
        failure(
            &adapter.call("agent.request", adapter.job_params()),
            "job_limit",
        );

        let (job, cancel) = &cancelled[0];
        adapter.reply(&cancel.id, json!({"cancelled": true, "job_id": job}));
        retained_capture(&adapter, 31);
        assert_eq!(adapter.session.state()["pending_jobs"], 0);
        assert_eq!(
            adapter.request_count("host.ask_agent"),
            1,
            "no automatic replay"
        );
        adapter.agent(); // A new explicit request can now consume the freed slot.
        assert_eq!(adapter.session.state()["pending_jobs"], 1);
        failure(
            &adapter.call("agent.request", adapter.job_params()),
            "job_limit",
        );
        for (job, _) in &cancelled {
            adapter.terminal(job, "cancelled");
        }
        adapter.terminal(&agent_job, "document_replaced");
        assert_eq!(adapter.request_count("host.capture_region"), 32);
        assert_eq!(adapter.request_count("host.ask_agent"), 2);
    }
}

#[test]
fn document_replacement_does_not_reset_host_job_identity_or_replay_writeback() {
    for app in [AppKind::Drawing, AppKind::Blackboard] {
        let mut adapter = Adapter::new(app);
        let (old_job, old_request) = adapter.agent();
        adapter.reply(&old_request.id, json!({"job_id": "host:used"}));
        let old_document = adapter.session.state()["document_id"].clone();
        success(&adapter.call("document.new", json!({})));
        assert_ne!(adapter.session.state()["document_id"], old_document);
        adapter.terminal(&old_job, "document_replaced");
        let document = adapter.session.document.clone();
        assert!(
            adapter
                .finish("host:used", &old_request.id, writeback())
                .is_empty()
        );
        assert_eq!(adapter.session.document, document);
        assert_eq!(adapter.request_count("host.ask_agent"), 1);

        let (reused_job, reused_request) = adapter.agent();
        adapter.reply(&reused_request.id, json!({"job_id": "host:used"}));
        adapter.terminal(&reused_job, "invalid_host_response");
        assert!(
            adapter
                .finish("host:used", &reused_request.id, writeback())
                .is_empty()
        );
        assert_eq!(adapter.session.document, document);

        // Positive control: the same payload is valid for fresh, authorized work.
        let (fresh_job, fresh_request) = adapter.agent();
        adapter.reply(&fresh_request.id, json!({"job_id": "host:fresh"}));
        let out = adapter.finish("host:fresh", &fresh_request.id, writeback());
        assert!(
            out.iter().any(|m| matches!(m, Message::Event(e)
            if e.event == "job.finished" && e.data["job_id"] == fresh_job && e.data["ok"] == true))
        );
        assert_eq!(adapter.session.state()["revision"], 1);
        let params = adapter.context();
        let objects = success(&adapter.call("objects.list", params));
        assert_eq!(objects["objects"].as_array().unwrap().len(), 1);
        assert_eq!(objects["objects"][0]["id"], "host-content");
        let before = adapter.session.document.clone();
        assert!(
            adapter
                .finish("host:used", &old_request.id, writeback())
                .is_empty()
        );
        assert!(
            adapter
                .finish("host:fresh", &fresh_request.id, writeback())
                .is_empty()
        );
        assert_eq!(adapter.session.document, before);
        adapter.terminal(&old_job, "document_replaced");
        adapter.terminal(&reused_job, "invalid_host_response");
    }
}

#[test]
fn dirty_close_preserves_jobs_and_discard_close_waits_for_all_window_observations() {
    for app in [AppKind::Drawing, AppKind::Blackboard] {
        let mut adapter = Adapter::new(app);
        adapter.windows();
        adapter.dirty();
        let (capture_job, capture) = adapter.capture();
        let (agent_job, agent) = adapter.agent();
        adapter.reply(&agent.id, json!({"job_id": "host:closing-agent"}));
        let document = adapter.session.document.clone();
        let before = adapter.session.state();
        for params in [json!({}), json!({"discard_unsaved": false})] {
            let out = adapter.call("close", params);
            failure(&out, "unsaved_changes");
            assert_eq!(out.len(), 1, "rejected close must not cancel jobs");
            assert_eq!(adapter.session.state(), before);
            assert_eq!(adapter.session.document, document);
        }
        assert_eq!(before["pending_jobs"], 2);
        assert_eq!(adapter.request_count("jobs.cancel"), 0);
        let out = adapter.send(
            "neo:discard-close",
            "close",
            json!({"discard_unsaved": true}),
        );
        assert!(!out.iter().any(|m| matches!(m, Message::Response(_))));
        adapter.terminal(&capture_job, "session_closed");
        adapter.terminal(&agent_job, "session_closed");
        let cancel = out
            .iter()
            .find_map(|m| match m {
                Message::Request(r)
                    if r.method == "jobs.cancel" && r.params["request_id"] == capture.id =>
                {
                    Some(r.clone())
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(adapter.session.state()["pending_jobs"], 0);
        assert_eq!(adapter.session.state()["close_pending"], true);
        assert_eq!(adapter.session.state()["closed"], false);
        retained_capture(&adapter, 1);
        let hide = adapter.session.pending_window_request().unwrap().clone();
        assert!(!hide.visible);
        let before = adapter.session.state();
        assert!(
            adapter
                .observe("stale-window-request", &[("main", false), ("tools", false)])
                .is_empty()
        );
        for observations in [
            vec![("main", false)],
            vec![("main", false), ("tools", true)],
            vec![("main", false), ("main", false)],
            vec![("main", false), ("unknown", false)],
        ] {
            assert!(adapter.observe(&hide.request_id, &observations).is_empty());
            assert_eq!(adapter.session.state(), before);
        }
        failure(&adapter.call("show", json!({})), "session_closing");
        assert!(
            adapter
                .finish("host:closing-agent", &agent.id, writeback())
                .is_empty()
        );
        let out = adapter.observe(&hide.request_id, &[("tools", false), ("main", false)]);
        assert_eq!(response(&out).id, "neo:discard-close");
        let closed = success(&out);
        assert_eq!(closed["closed"], true);
        assert_eq!(closed["close_pending"], false);
        assert_eq!(closed["hidden_confirmed"], true);
        assert_eq!(adapter.session.document, document);
        retained_capture(&adapter, 1); // Window hiding is not host capture-stop proof.
        assert!(
            adapter
                .observe(&hide.request_id, &[("main", false), ("tools", false)])
                .is_empty()
        );
        adapter.reply(
            &cancel.id,
            json!({"cancelled": true, "job_id": capture_job}),
        );
        assert_eq!(adapter.session.state()["hide_lease_count"], 0);
        assert!(!adapter.session.effective_visible());
        assert!(
            adapter
                .reply(&cancel.id, json!({"cancelled": true}))
                .is_empty()
        );
        assert!(adapter.reply(&agent.id, writeback()).is_empty());
        adapter.terminal(&capture_job, "session_closed");
        adapter.terminal(&agent_job, "session_closed");
        assert_eq!(adapter.session.document, document);
        assert_eq!(
            adapter
                .transcript
                .iter()
                .filter(|m| matches!(m,
            Message::Response(r) if r.id == "neo:discard-close"))
                .count(),
            1
        );
        assert_eq!(adapter.request_count("jobs.cancel"), 2);
        assert_eq!(adapter.request_count("host.capture_region"), 1);
        assert_eq!(adapter.request_count("host.ask_agent"), 1);
    }
}
