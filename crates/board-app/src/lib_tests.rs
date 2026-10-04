use super::*;
fn parse(args: &[&str]) -> Result<Launch> {
    parse_args(args.iter().map(|s| s.to_string()))
}
struct TempRecovery(std::path::PathBuf);
impl TempRecovery {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("board-app-test-{}", board_core::new_id())))
    }
}
impl Drop for TempRecovery {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn dirty_session() -> Session {
    let mut session = Session::new(AppKind::Drawing);
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[10, 20, 30, 255])
            .unwrap();
    }
    let asset_ref = session.resources.import_png(bytes).unwrap();
    let page = session.document.current_page().id.clone();
    session
        .history
        .apply(
            &mut session.document,
            &page,
            0,
            &[board_core::Operation::Add {
                object: board_core::BoardObject {
                    id: board_core::new_id(),
                    kind: board_core::ObjectKind::Image {
                        position: Default::default(),
                        width: 1.0,
                        height: 1.0,
                        asset_ref,
                    },
                },
            }],
        )
        .unwrap();
    session
}
fn assert_recovery(session: &Session, directory: &std::path::Path) {
    let paths: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|p| p.unwrap().path())
        .collect();
    assert_eq!(paths.len(), 1);
    let mut restored = Session::new(AppKind::Drawing);
    restored.open_document(&paths[0], false).unwrap();
    assert_eq!(restored.document, session.document);
    let board_core::ObjectKind::Image { asset_ref, .. } =
        &restored.document.current_page().objects[0].kind
    else {
        panic!()
    };
    assert_eq!(
        restored.resources.png_bytes(asset_ref),
        session.resources.png_bytes(asset_ref)
    );
}
#[test]
fn eof_recovers_document_and_embedded_image() {
    let directory = TempRecovery::new();
    let mut session = dirty_session();
    run_transport(
        &mut session,
        &mut io::Cursor::new([]),
        &mut Vec::new(),
        &directory.0,
    )
    .unwrap();
    assert_eq!(session.state()["connected"], false);
    assert_recovery(&session, &directory.0);
}
#[test]
fn broken_stdout_still_recovers() {
    struct Broken;
    impl io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let directory = TempRecovery::new();
    let mut session = dirty_session();
    assert!(
        run_transport(
            &mut session,
            &mut io::Cursor::new([]),
            &mut Broken,
            &directory.0
        )
        .is_err()
    );
    assert_recovery(&session, &directory.0);
}
#[test]
fn eof_does_not_confirm_capture_stopped() {
    use board_protocol::Request;
    use serde_json::json;
    let directory = TempRecovery::new();
    let mut session = Session::new(AppKind::Drawing);
    session.attach_window();
    session.handle(
        Request::new(
            "neo:config",
            "configure",
            json!({"classroom_safe":false,"desktop_capture_allowed":true,"agent_allowed":false}),
        )
        .unwrap(),
    );
    let request = session.pending_window_request().unwrap().clone();
    session.acknowledge_window(&request.request_id, request.visible);
    let state = session.state();
    session.handle(Request::new("neo:capture", "capture.request", json!({"document_id":state["document_id"],"page_id":state["page_id"],"expected_revision":state["revision"],"user_authorized":true})).unwrap());
    let request = session.pending_window_request().unwrap().clone();
    assert!(!request.visible);
    // 仅模拟窗口确认，绝不执行返回的 host.capture_region 请求。
    session.acknowledge_window(&request.request_id, false);
    run_transport(
        &mut session,
        &mut io::Cursor::new([]),
        &mut Vec::new(),
        &directory.0,
    )
    .unwrap();
    assert_eq!(session.state()["pending_jobs"], 0);
    assert_eq!(session.state()["pending_capture_cancellations"], 1);
    assert!(!session.effective_visible());
    assert_eq!(session.state()["hide_lease_count"], 1);
}
#[test]
fn recovery_failure_is_reported_without_marking_saved() {
    let directory = TempRecovery::new();
    std::fs::create_dir_all(&directory.0).unwrap();
    let blocked = directory.0.join("not-a-directory");
    std::fs::write(&blocked, b"test").unwrap();
    let mut session = dirty_session();
    assert!(
        run_transport(
            &mut session,
            &mut io::Cursor::new([]),
            &mut Vec::new(),
            &blocked
        )
        .is_err()
    );
    assert!(session.history.is_dirty(&session.document));
}
#[test]
fn clean_eof_does_not_create_recovery() {
    let directory = TempRecovery::new();
    run_transport(
        &mut Session::new(AppKind::Drawing),
        &mut io::Cursor::new([]),
        &mut Vec::new(),
        &directory.0,
    )
    .unwrap();
    assert!(!directory.0.exists());
}
#[test]
fn malformed_pending_completion_cancels_before_next_request() {
    use board_protocol::Request;
    use serde_json::json;
    for frame in [
        b"{bad\n".to_vec(),
        [vec![b'x'; 65_537], vec![b'\n']].concat(),
    ] {
        let directory = TempRecovery::new();
        let mut session = Session::new(AppKind::Drawing);
        session.handle(
            Request::new(
                "neo:config",
                "configure",
                json!({"classroom_safe":true,"desktop_capture_allowed":false,"agent_allowed":true}),
            )
            .unwrap(),
        );
        let state = session.state();
        session.handle(Request::new("neo:agent", "agent.request", json!({"document_id":state["document_id"],"page_id":state["page_id"],"expected_revision":state["revision"],"prompt":"test","asset_refs":[],"user_authorized":true,"write_back":false})).unwrap());
        assert_eq!(session.state()["pending_jobs"], 1);
        let mut input = frame;
        write_message(
            &mut input,
            &Request::new("neo:barrier", "get_state", json!({}))
                .unwrap()
                .into(),
        )
        .unwrap();
        let mut output = Vec::new();
        run_transport(
            &mut session,
            &mut io::Cursor::new(input),
            &mut output,
            &directory.0,
        )
        .unwrap();
        let messages: Vec<serde_json::Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert!(
            messages
                .iter()
                .any(|m| m["event"] == "job.finished" && m["data"]["ok"] == false)
        );
        let barrier = messages.iter().find(|m| m["id"] == "neo:barrier").unwrap();
        assert_eq!(barrier["result"]["pending_jobs"], 0);
    }
}
#[test]
fn default_never_launches_gui() {
    assert_eq!(parse(&[]).unwrap(), Launch::Help);
    assert_eq!(parse(&["--help"]).unwrap(), Launch::Help);
    assert_eq!(parse(&["--headless"]).unwrap(), Launch::Headless);
    assert_eq!(parse(&["--gui"]).unwrap(), Launch::Gui { hosted: false });
    assert_eq!(
        parse(&["--gui", "--hosted"]).unwrap(),
        Launch::Gui { hosted: true }
    );
}
#[test]
fn version_is_an_exclusive_non_gui_command() {
    for flag in ["--version", "-V"] {
        assert_eq!(parse(&[flag]).unwrap(), Launch::Version);
        assert!(parse(&[flag, "--gui"]).is_err());
        assert!(parse(&["--headless", flag]).is_err());
    }
}

#[test]
fn conflicting_flags_are_rejected() {
    for flags in [
        &["--hosted"][..],
        &["--gui", "--headless"],
        &["--gui", "--gui"],
        &["--unknown"],
    ] {
        assert!(parse(flags).is_err());
    }
}
