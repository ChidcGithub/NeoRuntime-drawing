use super::*;
use board_protocol::Request;
use board_session::AppKind;
use serde_json::{Value, json};
use std::io::{BufRead, Read};

struct TempRecovery(std::path::PathBuf);
impl TempRecovery {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("hosted-startup-test-{}", board_core::new_id())))
    }
}
impl Drop for TempRecovery {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// Source fixture: gui.rs at 8c32db0 attached a window, read until configured
// or closed, and only then attempted the hidden ACK. Before the adapter fix,
// close_before_configure_exits_without_eof failed with the panic below (not
// EOF or a GUI timeout): Session correctly remained close_pending until ACK.
// A host keeps stdin open after sending close. A read beyond its last frame
// would block in production; fail immediately instead, without starting a GUI.
struct HeldOpen(io::Cursor<Vec<u8>>);
impl Read for HeldOpen {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let count = available.len().min(bytes.len());
        bytes[..count].copy_from_slice(&available[..count]);
        self.consume(count);
        Ok(count)
    }
}
impl BufRead for HeldOpen {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        assert!(
            self.0.position() < self.0.get_ref().len() as u64,
            "startup read past close: held-open stdin would block"
        );
        self.0.fill_buf()
    }
    fn consume(&mut self, amount: usize) {
        self.0.consume(amount);
    }
}
fn request(id: &str, method: &str, params: Value) -> Request {
    Request::new(id, method, params).unwrap()
}
fn input(requests: Vec<Request>) -> HeldOpen {
    let mut bytes = Vec::new();
    for request in requests {
        write_message(&mut bytes, &request.into()).unwrap();
    }
    HeldOpen(io::Cursor::new(bytes))
}
fn attached() -> Session {
    let mut session = Session::new(AppKind::Drawing);
    session.attach_window();
    session
}
fn frames(output: &[u8]) -> Vec<Value> {
    std::str::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn close_before_configure_exits_without_eof() {
    let recovery = TempRecovery::new();
    let mut session = attached();
    let mut input = input(vec![request("neo:close", "close", json!({}))]);
    let mut output = Vec::new();
    assert_eq!(
        run(&mut session, &mut input, &mut output, &recovery.0).unwrap(),
        StartupOutcome::Closed
    );
    assert!(session.closed);
    assert!(!session.configured);
    let frames = frames(&output);
    assert_eq!(frames[0]["event"], "ready");
    let response = frames
        .iter()
        .find(|frame| frame["id"] == "neo:close")
        .unwrap();
    assert_eq!(response["ok"], true);
    assert_eq!(response["result"]["closed"], true);
    assert_eq!(response["result"]["document_id"], session.document.id);
    assert!(!recovery.0.exists());
}

fn configure() -> Request {
    request(
        "neo:configure",
        "configure",
        json!({
            "classroom_safe": true, "desktop_capture_allowed": false, "agent_allowed": false
        }),
    )
}
fn response<'a>(frames: &'a [Value], id: &str) -> &'a Value {
    let found: Vec<_> = frames.iter().filter(|frame| frame["id"] == id).collect();
    assert_eq!(found.len(), 1, "exactly one response for {id}");
    found[0]
}
fn assert_unread(input: &mut HeldOpen, id: &str) {
    let Some(Message::Request(request)) = read_message(input).unwrap() else {
        panic!("expected unread request");
    };
    assert_eq!(request.id, id);
}

#[test]
fn configure_then_close_leaves_close_and_visible_ack_for_gui() {
    let recovery = TempRecovery::new();
    let mut session = attached();
    let mut input = input(vec![configure(), request("neo:close", "close", json!({}))]);
    let mut output = Vec::new();
    assert_eq!(
        run(&mut session, &mut input, &mut output, &recovery.0).unwrap(),
        StartupOutcome::Configured
    );
    assert_unread(&mut input, "neo:close");
    let pending = session.pending_window_request().unwrap();
    assert!(pending.visible);
    assert_eq!(session.state()["visible"], false);
    assert_eq!(session.state()["window_status"], "pending");
    let frames = frames(&output);
    assert_eq!(frames.iter().filter(|f| f["event"] == "ready").count(), 1);
    assert_eq!(frames[0]["data"]["has_window"], true);
    assert_eq!(frames[0]["data"]["headless"], false);
    let configured = response(&frames, "neo:configure");
    assert_eq!(configured["result"]["document_id"], session.document.id);
    assert_eq!(
        configured["result"]["page_id"],
        session.document.current_page().id
    );
    assert!(!frames.iter().any(|f| f["id"] == "neo:close"));
    assert!(frames.iter().any(|f| f["event"] == "window.requested"
        && f["data"]["request_id"] == pending.request_id
        && f["data"]["visible"] == true));
    assert!(!recovery.0.exists());
}

#[test]
fn close_then_configure_never_reaches_native_creation_boundary() {
    let recovery = TempRecovery::new();
    let mut session = attached();
    let mut input = input(vec![request("neo:close", "close", json!({})), configure()]);
    let mut output = Vec::new();
    let outcome = run(&mut session, &mut input, &mut output, &recovery.0).unwrap();
    // gui::run returns on Closed; only Configured reaches NativeOptions/run_native.
    assert_eq!(outcome, StartupOutcome::Closed);
    assert_unread(&mut input, "neo:configure");
    assert!(!session.configured);
    assert!(session.pending_window_request().is_none());
    let frames = frames(&output);
    assert_eq!(
        response(&frames, "neo:close")["result"]["hidden_confirmed"],
        true
    );
    assert!(!frames.iter().any(|f| f["id"] == "neo:configure"));
}

#[test]
fn repeated_close_is_terminal_once() {
    let recovery = TempRecovery::new();
    let mut session = attached();
    let mut input = input(vec![
        request("neo:first", "close", json!({})),
        request("neo:second", "close", json!({})),
    ]);
    let mut output = Vec::new();
    assert_eq!(
        run(&mut session, &mut input, &mut output, &recovery.0).unwrap(),
        StartupOutcome::Closed
    );
    let emitted = output.len();
    assert_eq!(
        run(&mut session, &mut input, &mut output, &recovery.0).unwrap(),
        StartupOutcome::Closed
    );
    assert_eq!(output.len(), emitted);
    assert_unread(&mut input, "neo:second");
    let frames = frames(&output);
    assert_eq!(response(&frames, "neo:first")["ok"], true);
    assert!(!frames.iter().any(|f| f["id"] == "neo:second"));
}

fn dirty_session() -> Session {
    let mut session = attached();
    let page = session.document.current_page().id.clone();
    // Direct core setup is deliberate: document mutation RPCs require configure.
    session
        .history
        .apply(
            &mut session.document,
            &page,
            0,
            &[board_core::Operation::Add {
                object: board_core::BoardObject {
                    id: board_core::new_id(),
                    kind: board_core::ObjectKind::Text {
                        position: Default::default(),
                        text: "unsaved startup fixture".into(),
                        size: 20.0,
                        color: Default::default(),
                    },
                },
            }],
        )
        .unwrap();
    assert!(!session.configured);
    assert!(session.history.is_dirty(&session.document));
    session
}

#[test]
fn dirty_close_rejection_continues_until_explicit_discard() {
    let recovery = TempRecovery::new();
    let mut session = dirty_session();
    let mut input = input(vec![
        request("neo:reject", "close", json!({})),
        request("neo:state", "get_state", json!({})),
        request("neo:discard", "close", json!({"discard_unsaved": true})),
    ]);
    let mut output = Vec::new();
    assert_eq!(
        run(&mut session, &mut input, &mut output, &recovery.0).unwrap(),
        StartupOutcome::Closed
    );
    let frames = frames(&output);
    assert_eq!(
        response(&frames, "neo:reject")["error"]["code"],
        "unsaved_changes"
    );
    let state = &response(&frames, "neo:state")["result"];
    assert_eq!(state["dirty"], true);
    assert_eq!(state["close_pending"], false);
    assert_eq!(state["hidden_confirmed"], false);
    assert_eq!(response(&frames, "neo:discard")["result"]["closed"], true);
    assert!(session.history.is_dirty(&session.document));
    assert!(!recovery.0.exists());
}

#[test]
fn dirty_rejection_allows_configure_then_explicit_local_save() {
    let recovery = TempRecovery::new();
    let mut session = dirty_session();
    let mut input = input(vec![request("neo:reject", "close", json!({})), configure()]);
    let mut output = Vec::new();
    assert_eq!(
        run(&mut session, &mut input, &mut output, &recovery.0).unwrap(),
        StartupOutcome::Configured
    );
    assert!(session.history.is_dirty(&session.document));
    assert!(
        !recovery.0.exists(),
        "configure must not autosave dirty content"
    );
    assert_eq!(
        response(&frames(&output), "neo:reject")["error"]["code"],
        "unsaved_changes"
    );
    std::fs::create_dir_all(&recovery.0).unwrap();
    session
        .save_document(recovery.0.join("explicit-save.neoboard"))
        .unwrap();
    let messages = session.handle(request("neo:saved-close", "close", json!({})));
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::Response(r) if r.id == "neo:saved-close"))
    );
    assert!(
        !session.closed,
        "configured close still requires the GUI ACK"
    );
    let pending = session.pending_window_request().unwrap().clone();
    assert!(!pending.visible);
    session.acknowledge_window(&pending.request_id, false);
    assert!(session.closed);
}

#[test]
fn explicit_local_save_before_configure_allows_close_without_discard() {
    let directory = TempRecovery::new();
    std::fs::create_dir_all(&directory.0).unwrap();
    let mut session = dirty_session();
    session
        .save_document(directory.0.join("explicit-save.neoboard"))
        .unwrap();
    let mut output = Vec::new();
    assert_eq!(
        run(
            &mut session,
            &mut input(vec![request("neo:close", "close", json!({}))]),
            &mut output,
            &directory.0.join("recovery")
        )
        .unwrap(),
        StartupOutcome::Closed
    );
    assert_eq!(response(&frames(&output), "neo:close")["ok"], true);
    assert!(!directory.0.join("recovery").exists());
}

fn assert_recovered(session: &Session, directory: &Path) {
    let paths: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|p| p.unwrap().path())
        .collect();
    assert_eq!(paths.len(), 1);
    let mut restored = Session::new(AppKind::Drawing);
    restored.open_document(&paths[0], false).unwrap();
    assert_eq!(restored.document, session.document);
    assert_eq!(session.state()["connected"], false);
}

#[test]
fn eof_recovers_dirty_preconfigure_document_in_injected_directory() {
    let recovery = TempRecovery::new();
    let mut session = dirty_session();
    assert_eq!(
        run(
            &mut session,
            &mut io::Cursor::new([]),
            &mut Vec::new(),
            &recovery.0
        )
        .unwrap(),
        StartupOutcome::Disconnected
    );
    assert_recovered(&session, &recovery.0);
    assert!(!session.closed);
    assert_eq!(session.state()["hidden_confirmed"], false);
}

#[test]
fn clean_eof_does_not_ack_attach_or_create_recovery() {
    let recovery = TempRecovery::new();
    let mut session = attached();
    assert_eq!(
        run(
            &mut session,
            &mut io::Cursor::new([]),
            &mut Vec::new(),
            &recovery.0
        )
        .unwrap(),
        StartupOutcome::Disconnected
    );
    assert!(session.pending_window_request().is_some());
    assert_eq!(session.state()["hidden_confirmed"], false);
    assert!(!recovery.0.exists());
}

#[test]
fn configured_session_guard_never_acks_visible_or_pending_close() {
    for closing in [false, true] {
        let recovery = TempRecovery::new();
        let mut session = attached();
        session.handle(configure());
        if closing {
            session.handle(request("neo:close", "close", json!({})));
        }
        let before = session.state();
        let pending_id = session.pending_window_request().unwrap().request_id.clone();
        let mut output = Vec::new();
        // Any read panics; this is not a second startup handshake.
        assert_eq!(
            run(&mut session, &mut input(vec![]), &mut output, &recovery.0).unwrap(),
            StartupOutcome::Configured
        );
        assert_eq!(session.state(), before);
        assert_eq!(
            session.pending_window_request().unwrap().request_id,
            pending_id
        );
        assert!(output.is_empty());
        assert!(!recovery.0.exists());
    }
}

#[test]
fn configured_capture_cancellation_is_not_synthetically_confirmed() {
    let recovery = TempRecovery::new();
    let mut session = attached();
    session.handle(request(
        "neo:config",
        "configure",
        json!({
            "classroom_safe": false, "desktop_capture_allowed": true, "agent_allowed": false
        }),
    ));
    let visible = session.pending_window_request().unwrap().clone();
    session.acknowledge_window(&visible.request_id, true);
    let state = session.state();
    session.handle(request(
        "neo:capture",
        "capture.request",
        json!({
            "document_id": state["document_id"], "page_id": state["page_id"],
            "expected_revision": state["revision"], "user_authorized": true
        }),
    ));
    let hidden = session.pending_window_request().unwrap().clone();
    // Only Session messages: never execute the returned host.capture_region.
    session.acknowledge_window(&hidden.request_id, false);
    session.handle(request("neo:close", "close", json!({})));
    assert_eq!(session.state()["pending_capture_cancellations"], 1);
    let before = session.state();
    assert_eq!(
        run(
            &mut session,
            &mut input(vec![]),
            &mut Vec::new(),
            &recovery.0
        )
        .unwrap(),
        StartupOutcome::Configured
    );
    assert_eq!(session.state(), before);
    assert_eq!(session.state()["hide_lease_count"], 1);
    assert!(!session.closed);
}

#[test]
fn malformed_input_reports_error_and_continues_to_close() {
    let recovery = TempRecovery::new();
    let mut session = attached();
    let close = input(vec![request("neo:close", "close", json!({}))]);
    let mut bytes = b"not json\n".to_vec();
    bytes.extend(close.0.into_inner());
    let mut output = Vec::new();
    assert_eq!(
        run(
            &mut session,
            &mut HeldOpen(io::Cursor::new(bytes)),
            &mut output,
            &recovery.0
        )
        .unwrap(),
        StartupOutcome::Closed
    );
    let frames = frames(&output);
    assert!(
        frames
            .iter()
            .any(|f| f["event"] == "protocol_error" && f["data"]["code"] == "invalid_json")
    );
    assert_eq!(response(&frames, "neo:close")["ok"], true);
}

#[test]
fn broken_output_still_recovers_dirty_document() {
    struct Broken;
    impl io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let recovery = TempRecovery::new();
    let mut session = dirty_session();
    let error = run(&mut session, &mut input(vec![]), &mut Broken, &recovery.0).unwrap_err();
    assert!(error.to_string().contains("pipe"));
    assert_recovered(&session, &recovery.0);
}

#[test]
fn input_io_error_still_recovers_dirty_document() {
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::ConnectionReset.into())
        }
    }
    impl BufRead for Broken {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            Err(io::ErrorKind::ConnectionReset.into())
        }
        fn consume(&mut self, _: usize) {}
    }
    let recovery = TempRecovery::new();
    let mut session = dirty_session();
    let mut output = Vec::new();
    assert!(run(&mut session, &mut Broken, &mut output, &recovery.0).is_err());
    assert!(
        frames(&output)
            .iter()
            .any(|f| f["event"] == "protocol_error" && f["data"]["code"] == "io_error")
    );
    assert_recovered(&session, &recovery.0);
}

#[test]
fn recovery_failure_is_reported_and_document_stays_dirty() {
    let directory = TempRecovery::new();
    std::fs::create_dir_all(&directory.0).unwrap();
    let blocked = directory.0.join("not-a-directory");
    std::fs::write(&blocked, b"fixture").unwrap();
    let mut session = dirty_session();
    assert!(
        run(
            &mut session,
            &mut io::Cursor::new([]),
            &mut Vec::new(),
            &blocked
        )
        .is_err()
    );
    assert!(session.history.is_dirty(&session.document));
    assert!(!session.closed);
}
