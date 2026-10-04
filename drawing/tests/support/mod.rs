use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

mod scenarios;

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "neo-headless-test-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const TIMEOUT: Duration = Duration::from_secs(8);
type Input = (Vec<u8>, Sender<std::io::Result<()>>);

struct Process {
    child: Child,
    input: Option<Sender<Input>>,
    output: Receiver<Result<Vec<u8>, String>>,
    errors: Receiver<Vec<u8>>,
    threads: Vec<JoinHandle<()>>,
    pending: Vec<Value>,
    sequence: u64,
    directory: TempDirectory,
    break_output: Arc<AtomicBool>,
    output_closed: Receiver<()>,
}

impl Process {
    fn spawn(args: &[&str]) -> Self {
        let directory = TempDirectory::new();
        let mut child = Command::new(super::BINARY)
            .env("LOCALAPPDATA", &directory.0)
            .current_dir(&directory.0)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("启动 Cargo 当前构建的二进制");
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let (input, writes) = mpsc::channel::<Input>();
        let writer = thread::spawn(move || {
            while let Ok((bytes, reply)) = writes.recv() {
                let result = stdin.write_all(&bytes).and_then(|_| stdin.flush());
                let failed = result.is_err();
                let _ = reply.send(result);
                if failed {
                    break;
                }
            }
        });
        let (lines, output) = mpsc::channel();
        let break_output = Arc::new(AtomicBool::new(false));
        let reader_break = Arc::clone(&break_output);
        let (closed, output_closed) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = Vec::new();
                match reader.by_ref().take(65_538).read_until(b'\n', &mut line) {
                    Ok(0) => break,
                    Ok(_) if line.last() == Some(&b'\n') && line.len() <= 65_537 => {
                        let disconnect = reader_break.load(Ordering::Acquire);
                        if lines.send(Ok(line)).is_err() || disconnect {
                            break;
                        }
                    }
                    Ok(_) => {
                        let _ = lines.send(Err("stdout 帧超长或缺少换行".into()));
                        break;
                    }
                    Err(error) => {
                        let _ = lines.send(Err(error.to_string()));
                        break;
                    }
                }
            }
            drop(reader);
            let _ = closed.send(());
        });
        let (diagnostics, errors) = mpsc::channel();
        let error_reader = thread::spawn(move || {
            let mut captured = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                match stderr.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let keep = count.min(65_536 - captured.len());
                        captured.extend_from_slice(&buffer[..keep]);
                    }
                }
            }
            let _ = diagnostics.send(captured);
        });
        Self {
            child,
            input: Some(input),
            output,
            errors,
            threads: vec![writer, reader, error_reader],
            pending: Vec::new(),
            sequence: 0,
            directory,
            break_output,
            output_closed,
        }
    }

    fn headless() -> Self {
        let process = Self::spawn(&["--headless"]);
        let ready = process.receive(Instant::now() + TIMEOUT);
        assert_eq!(ready["type"], "event");
        assert_eq!(ready["event"], "ready");
        assert_eq!(ready["data"]["app"], super::APP);
        assert_eq!(ready["data"]["headless"], true);
        assert_eq!(ready["data"]["has_window"], false);
        assert_eq!(ready["data"]["max_line_bytes"], 65_536);
        let methods = ready["data"]["methods"].as_array().unwrap();
        for method in [
            "configure",
            "get_state",
            "objects.list",
            "objects.apply",
            "undo",
            "redo",
            "agent.request",
            "close",
        ] {
            assert!(methods.contains(&json!(method)), "ready 未声明 {method}");
        }
        assert!(!methods.contains(&json!("capture.request")));
        assert!(
            !ready["data"]["host_methods"]
                .as_array()
                .unwrap()
                .contains(&json!("host.capture_region"))
        );
        for event in ["job.finished", "jobs.finished"] {
            assert!(
                ready["data"]["host_events"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(event))
            );
        }
        process
    }

    fn raw(&self, bytes: Vec<u8>) {
        let (reply, result) = mpsc::channel();
        self.input.as_ref().unwrap().send((bytes, reply)).unwrap();
        result
            .recv_timeout(TIMEOUT)
            .expect("写入 stdin 超时")
            .expect("写入 stdin 失败");
    }

    fn send(&self, message: Value) {
        let mut bytes = serde_json::to_vec(&message).unwrap();
        bytes.push(b'\n');
        self.raw(bytes);
    }

    fn parse(line: Result<Vec<u8>, String>) -> Value {
        let bytes = line.expect("stdout 必须为有界 JSON Lines");
        let value: Value = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            panic!(
                "stdout 含非协议内容：{error}，{:?}",
                String::from_utf8_lossy(&bytes)
            )
        });
        assert_eq!(value["version"], 1);
        match value["type"].as_str() {
            Some("event") => {
                assert!(value["event"].is_string());
                assert!(value.get("data").is_some());
            }
            Some("response") => {
                assert!(value["id"].as_str().unwrap().starts_with("neo:"));
                let ok = value["ok"].as_bool().unwrap();
                assert_eq!(value.get("result").is_some(), ok);
                assert_eq!(value.get("error").is_some(), !ok);
            }
            Some("request") => {
                assert!(value["id"].as_str().unwrap().starts_with("runtime:"));
                assert_eq!(
                    value["method"], "host.ask_agent",
                    "测试不得请求采集或其他宿主副作用"
                );
                assert!(value["params"].is_object());
            }
            _ => panic!("stdout 含未知消息：{value}"),
        }
        value
    }

    fn receive(&self, deadline: Instant) -> Value {
        let line = self
            .output
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("等待 stdout 超时或进程提前退出");
        Self::parse(line)
    }

    fn matching(&mut self, predicate: impl Fn(&Value) -> bool) -> Value {
        if let Some(index) = self.pending.iter().position(&predicate) {
            return self.pending.remove(index);
        }
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let message = self.receive(deadline);
            if predicate(&message) {
                return message;
            }
            assert!(self.pending.len() < 1024, "过多未处理消息");
            self.pending.push(message);
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        self.sequence += 1;
        let id = format!("neo:test-{}", self.sequence);
        self.send(
            json!({"version": 1, "type": "request", "id": id, "method": method, "params": params}),
        );
        self.matching(|v| v["type"] == "response" && v["id"] == id)
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let response = self.call(method, params);
        assert_eq!(response["ok"], true, "{method}: {response}");
        response["result"].clone()
    }

    fn configure(&mut self, agent: bool) -> Value {
        let state = self.ok("configure", json!({"classroom_safe": true, "desktop_capture_allowed": false, "agent_allowed": agent}));
        assert_eq!(state["configured"], true);
        assert_eq!(state["visible"], false);
        assert_eq!(state["has_window"], false);
        assert_eq!(state["permissions"]["desktop_capture_allowed"], false);
        state
    }

    fn finish(&mut self) -> (ExitStatus, Vec<Value>, Vec<u8>) {
        let deadline = Instant::now() + TIMEOUT;
        let status = loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "进程未按时退出");
            thread::sleep(Duration::from_millis(10));
        };
        self.input.take();
        let mut messages = std::mem::take(&mut self.pending);
        loop {
            match self
                .output
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(line) => messages.push(Self::parse(line)),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(error) => panic!("stdout 未关闭：{error}"),
            }
        }
        let errors = self.errors.recv_timeout(TIMEOUT).expect("stderr 未关闭");
        (status, messages, errors)
    }

    fn close(&mut self, discard: bool) {
        let state = self.ok("close", json!({"discard_unsaved": discard}));
        assert_eq!(state["closed"], true);
        assert_eq!(state["visible"], false);
        let (status, messages, errors) = self.finish();
        assert!(
            status.success(),
            "{status}: {}",
            String::from_utf8_lossy(&errors)
        );
        assert!(
            messages.iter().all(|v| v["type"] == "event"),
            "未消费的响应/请求：{messages:?}"
        );
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

fn context(state: &Value) -> Value {
    json!({"document_id": state["document_id"], "page_id": state["page_id"], "expected_revision": state["revision"]})
}

fn object(id: &str) -> Value {
    json!({"id": id, "kind": {"type": "text", "position": {"x": 10.0, "y": 20.0},
        "text": "集成测试，不写文件", "size": 20.0, "color": {"r": 0, "g": 0, "b": 0, "a": 255}}})
}

fn add(process: &mut Process, state: &Value, id: &str) -> Value {
    let mut params = context(state);
    params["operations"] = json!([{"op": "add", "object": object(id)}]);
    process.ok("objects.apply", params)
}

fn error(response: Value, code: &str) {
    assert_eq!(response["ok"], false, "{response}");
    assert_eq!(response["error"]["code"], code, "{response}");
}

#[test]
fn ready_configure_and_headless_visibility() {
    let mut process = Process::headless();
    let before = process.ok("get_state", json!({}));
    assert_eq!(before["app"], super::APP);
    assert_eq!(before["configured"], false);
    assert_eq!(before["dirty"], false);
    assert_eq!(before["visible"], false);
    assert_eq!(before["revision"], 0);
    assert!(!before["document_id"].as_str().unwrap().is_empty());
    assert!(!before["page_id"].as_str().unwrap().is_empty());
    error(process.call("show", json!({})), "not_configured");
    error(
        process.call("configure", json!({"classroom_safe": true})),
        "invalid_params",
    );
    assert_eq!(process.ok("get_state", json!({})), before);
    let configured = process.configure(false);
    assert_eq!(configured["document_id"], before["document_id"]);
    assert_eq!(configured["page_id"], before["page_id"]);
    for method in ["hide", "show"] {
        let state = process.ok(method, json!({}));
        assert_eq!(state["visible"], false);
        assert_eq!(state["window_status"], "no_window");
        assert_eq!(state["hidden_confirmed"], false);
    }
    process.close(false);
}

#[test]
fn atomic_objects_conflict_undo_redo_and_dirty_close() {
    let mut process = Process::headless();
    let initial = process.configure(false);
    let edited = add(&mut process, &initial, "first");
    assert_eq!(edited["revision"], 1);
    assert_eq!(edited["dirty"], true);
    assert_eq!(edited["can_undo"], true);
    let changed = process.matching(|v| v["event"] == "document_changed");
    assert_eq!(changed["data"]["revision"], edited["revision"]);
    let mut stale = context(&initial);
    stale["operations"] = json!([{"op": "add", "object": object("stale")}]);
    error(process.call("objects.apply", stale), "revision_conflict");
    let mut invalid = context(&edited);
    invalid["operations"] = json!([
        {"op": "add", "object": object("must-roll-back")},
        {"op": "delete", "id": "does-not-exist"}
    ]);
    error(process.call("objects.apply", invalid), "invalid_document");
    assert_eq!(process.ok("get_state", json!({})), edited);
    assert_eq!(
        process.ok("objects.list", context(&edited))["objects"],
        json!([object("first")])
    );
    let undo = process.ok("undo", context(&edited));
    assert_eq!(undo["changed"], true);
    assert_eq!(undo["state"]["revision"], 2);
    assert_eq!(undo["state"]["dirty"], false);
    assert_eq!(undo["state"]["can_redo"], true);
    assert_eq!(
        process.ok("objects.list", context(&undo["state"]))["objects"],
        json!([])
    );
    let redo = process.ok("redo", context(&undo["state"]));
    assert_eq!(redo["changed"], true);
    assert_eq!(redo["state"]["revision"], 3);
    assert_eq!(redo["state"]["dirty"], true);
    assert_eq!(
        process.ok("objects.list", context(&redo["state"]))["objects"],
        json!([object("first")])
    );
    error(process.call("close", json!({})), "unsaved_changes");
    assert_eq!(process.ok("get_state", json!({})), redo["state"]);
    process.close(true);
}

#[test]
fn malformed_lines_recover_without_stdout_pollution() {
    let mut process = Process::headless();
    let initial = process.configure(false);
    let cases = [
        (b"{broken json\n".to_vec(), "invalid_json"),
        (vec![0xff, b'\n'], "invalid_json"),
        (b"{\"version\":1,\"type\":\"unknown\"}\r\n".to_vec(), "invalid_message"),
        (b"{\"version\":2,\"type\":\"event\",\"event\":\"ignored\",\"data\":{}}\n".to_vec(), "unsupported_version"),
        (b"{\"version\":1,\"type\":\"request\",\"id\":\"bad\",\"method\":\"get_state\",\"params\":{}}\n".to_vec(), "invalid_id"),
        ([vec![b'x'; 65_537], vec![b'\n']].concat(), "line_too_long"),
    ];
    for (line, code) in cases {
        process.raw(line);
        let event = process.matching(|v| v["event"] == "protocol_error");
        assert_eq!(event["data"]["code"], code);
        assert_eq!(process.ok("get_state", json!({})), initial);
    }
    process.close(false);
}

fn agent_event(event_name: &str, stale: bool) {
    let mut process = Process::headless();
    let initial = process.configure(true);
    let mut params = context(&initial);
    params["prompt"] = json!("仅使用测试宿主返回，不连接网络");
    params["asset_refs"] = json!([]);
    params["user_authorized"] = json!(true);
    params["write_back"] = json!(true);
    let accepted = process.ok("agent.request", params);
    let request = process.matching(|v| v["type"] == "request" && v["method"] == "host.ask_agent");
    assert_eq!(request["params"]["job_id"], accepted["job_id"]);
    assert_eq!(request["params"]["revision"], initial["revision"]);
    assert_eq!(request["params"]["document_id"], initial["document_id"]);
    assert_eq!(request["params"]["page_id"], initial["page_id"]);
    process.send(
        json!({"version": 1, "type": "response", "id": request["id"], "ok": true,
        "result": {"job_id": "host:test-job", "status": "pending"}}),
    );
    assert_eq!(process.ok("get_state", json!({}))["pending_jobs"], 1);
    let completion = json!({"version": 1, "type": "event", "event": event_name,
        "data": {"job_id": "host:test-job", "request_id": request["id"], "ok": true,
            "result": {"answer": "测试回答", "operations": [{"op": "add", "object": object("agent-result")}]}}});
    let mut unrelated = completion.clone();
    unrelated["data"]["request_id"] = json!("runtime:unrelated");
    process.send(unrelated);
    assert_eq!(process.ok("get_state", json!({}))["pending_jobs"], 1);
    if stale {
        add(&mut process, &initial, "local-edit");
    }
    process.send(completion.clone());
    // get_state 是输入处理屏障：完成事件必须先于后续请求生效，不能永远挂起任务。
    let state = process.ok("get_state", json!({}));
    assert_eq!(
        state["pending_jobs"], 0,
        "headless 必须将宿主 {event_name} 转交会话处理"
    );
    let finished = process
        .matching(|v| v["event"] == "job.finished" && v["data"]["job_id"] == accepted["job_id"]);
    assert_eq!(finished["data"]["ok"], !stale);
    if stale {
        assert_eq!(finished["data"]["error"]["code"], "revision_conflict");
    } else {
        assert_eq!(finished["data"]["result"]["answer"], "测试回答");
    }
    assert_eq!(state["revision"], 1);
    assert_eq!(
        process.ok("objects.list", context(&state))["objects"],
        json!([object(if stale { "local-edit" } else { "agent-result" })])
    );
    process.send(completion);
    assert_eq!(
        process.ok("get_state", json!({})),
        state,
        "重复完成事件不得再次写回"
    );
    assert!(
        !process.pending.iter().any(|v| v["event"] == "job.finished"),
        "重复完成事件不得再次通知"
    );
    process.close(true);
}

#[test]
fn host_agent_async_event_applies_once() {
    agent_event("job.finished", false);
}

#[test]
fn host_agent_async_alias_applies_once() {
    agent_event("jobs.finished", false);
}

#[test]
fn host_agent_async_event_rejects_stale_writeback() {
    agent_event("job.finished", true);
}

#[test]
fn version_exits_without_stdout_or_gui() {
    for flag in ["--version", "-V"] {
        let mut process = Process::spawn(&[flag]);
        let (status, messages, errors) = process.finish();
        assert!(status.success());
        assert!(messages.is_empty(), "版本必须写入 stderr");
        let version = String::from_utf8(errors).unwrap();
        let title = if super::APP == "drawing" {
            "Neo 画板"
        } else {
            "Neo 黑板"
        };
        assert_eq!(
            version.trim(),
            format!("{title} {}", env!("CARGO_PKG_VERSION"))
        );
    }
}

#[test]
fn default_help_and_invalid_cli_exit_without_stdout_or_gui() {
    for args in [&[][..], &["--help"][..], &["-h"][..]] {
        let mut process = Process::spawn(args);
        let (status, messages, errors) = process.finish();
        assert!(status.success(), "{args:?}: {status}");
        assert!(messages.is_empty(), "帮助必须写到 stderr");
        let help = String::from_utf8(errors).unwrap();
        assert!(help.contains("--headless"));
        assert!(help.contains("--gui"));
    }
    for args in [
        &["--unknown"][..],
        &["--hosted"][..],
        &["--headless", "--headless"][..],
        &["--headless", "--hosted"][..],
        &["--gui", "--headless"][..],
    ] {
        let mut process = Process::spawn(args);
        let (status, messages, errors) = process.finish();
        assert!(!status.success(), "错误 CLI 必须失败：{args:?}");
        assert!(
            messages.is_empty(),
            "错误 CLI 不得输出协议帧或日志到 stdout"
        );
        assert!(!errors.is_empty(), "错误说明必须写 stderr");
    }
}
