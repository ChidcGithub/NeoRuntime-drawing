use super::*;
use serde_json::json;
use std::io::{BufReader, Cursor, Read};

fn request() -> Message {
    Request::new("neo:1", "configure", json!({"中文": "文字\n下一行"}))
        .unwrap()
        .into()
}

fn frame(message: &Message) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_message(&mut bytes, message).unwrap();
    bytes
}

fn parse(value: Value) -> Result<Option<Message>, TransportError> {
    read_message(&mut Cursor::new(serde_json::to_vec(&value).unwrap()))
}

#[test]
fn all_messages_round_trip_with_exact_wire_tags() {
    let mut error = ProtocolError::new(error_codes::INVALID_PARAMS, "参数错误");
    error.data = Some(json!({"field": "page_id"}));
    let messages = [
        request(),
        Response::success("runtime:1", Value::Null).unwrap().into(),
        Response::success("neo:1", json!([1, true, "x"]))
            .unwrap()
            .into(),
        Response::failure("neo:1", error).unwrap().into(),
        Event::new("ready", Value::Null).into(),
    ];
    for (message, kind) in messages
        .iter()
        .zip(["request", "response", "response", "response", "event"])
    {
        let bytes = frame(message);
        assert_eq!(bytes.iter().filter(|b| **b == b'\n').count(), 1);
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["type"], kind);
        assert_eq!(serde_json::from_slice::<Message>(&bytes).unwrap(), *message);
        let mut reader = BufReader::with_capacity(1, Cursor::new(bytes));
        assert_eq!(read_message(&mut reader).unwrap(), Some(message.clone()));
        assert_eq!(read_message(&mut reader).unwrap(), None);
    }
}

#[test]
fn direct_struct_serialization_includes_type_and_omits_absent_fields() {
    let response = Response::success("neo:1", Value::Null).unwrap();
    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(value["type"], "response");
    assert!(value.get("result").is_some_and(Value::is_null));
    assert!(!value.as_object().unwrap().contains_key("error"));
    let response: Response = serde_json::from_value(value).unwrap();
    assert_eq!(response.result, Some(Value::Null));
    let value = serde_json::to_value(
        Response::failure("neo:1", ProtocolError::new("custom", "failure")).unwrap(),
    )
    .unwrap();
    assert!(value.get("result").is_none());
    assert!(value["error"].get("data").is_none());
}

#[test]
fn unknown_fields_are_compatible_for_every_message_and_error() {
    for mut value in [
        json!({"version":1,"type":"request","id":"neo:1","method":"future.method","params":null}),
        json!({"version":1,"type":"response","id":"neo:1","ok":true,"result":null}),
        json!({"version":1,"type":"response","id":"neo:1","ok":false,"error":{"code":"custom","message":"failed","future":42}}),
        json!({"version":1,"type":"event","event":"future.event","data":null}),
    ] {
        value["future"] = json!({"nested": [1, 2]});
        assert!(parse(value).unwrap().is_some());
    }
}

#[test]
fn rejects_bad_versions_in_every_message() {
    for kind in ["request", "response", "event"] {
        for version in [
            json!(0),
            json!(2),
            json!(-1),
            json!(1.0),
            json!("1"),
            Value::Null,
            json!(true),
        ] {
            let value = json!({"type":kind,"version":version});
            assert!(matches!(
                parse(value),
                Err(TransportError::UnsupportedVersion)
            ));
        }
        assert!(matches!(
            parse(json!({"type":kind})),
            Err(TransportError::UnsupportedVersion)
        ));
    }
}

#[test]
fn rejects_missing_wrong_type_and_malformed_ids() {
    for kind in ["request", "response"] {
        let base =
            json!({"version":1,"type":kind,"method":"x","params":{},"ok":true,"result":null});
        assert!(matches!(
            parse(base.clone()),
            Err(TransportError::InvalidId)
        ));
        for id in [
            json!(null),
            json!(1),
            json!(true),
            json!([]),
            json!({}),
            json!(""),
            json!("other:1"),
            json!("neo:"),
            json!("runtime:"),
            json!("Neo:1"),
            json!("neo: "),
            json!("runtime:a\nb"),
            json!("neo:\u{0000}"),
            json!("neo:\u{2003}"),
        ] {
            let mut value = base.clone();
            value["id"] = id;
            assert!(matches!(parse(value), Err(TransportError::InvalidId)));
        }
    }
    for id in ["neo:", "bad", "runtime: "] {
        assert!(Request::new(id, "x", Value::Null).is_err());
        assert!(Response::success(id, Value::Null).is_err());
        assert!(Response::failure(id, ProtocolError::new("x", "x")).is_err());
    }
    for id in ["neo:1", "runtime:uuid-123", "neo:中文"] {
        assert!(Request::new(id, "x", Value::Null).is_ok());
    }
}

#[test]
fn rejects_bad_shape_and_missing_required_fields() {
    for value in [
        json!([]),
        json!(null),
        json!(42),
        json!({"version":1}),
        json!({"version":1,"type":"bogus"}),
        json!({"version":1,"type":1}),
        json!({"version":1,"type":"request","id":"neo:1","params":{}}),
        json!({"version":1,"type":"request","id":"neo:1","method":"x"}),
        json!({"version":1,"type":"request","id":"neo:1","method":null,"params":{}}),
        json!({"version":1,"type":"event","event":"x"}),
        json!({"version":1,"type":"event","data":{}}),
        json!({"version":1,"type":"event","event":1,"data":{}}),
    ] {
        assert!(matches!(
            parse(value),
            Err(TransportError::InvalidMessage(_))
        ));
    }
}

#[test]
fn response_body_must_match_ok_and_have_exactly_one_branch() {
    for body in [
        json!({"ok":true}),
        json!({"ok":false}),
        json!({"result":null}),
        json!({"ok":"true","result":null}),
        json!({"ok":false,"result":null}),
        json!({"ok":true,"error":{"code":"x","message":"x"}}),
        json!({"ok":true,"result":null,"error":null}),
        json!({"ok":false,"result":null,"error":{"code":"x","message":"x"}}),
        json!({"ok":false,"error":null}),
        json!({"ok":false,"error":"x"}),
        json!({"ok":false,"error":{}}),
        json!({"ok":false,"error":{"code":4,"message":"x"}}),
        json!({"ok":false,"error":{"code":"x"}}),
    ] {
        let mut value = json!({"version":1,"type":"response","id":"neo:1"});
        value
            .as_object_mut()
            .unwrap()
            .extend(body.as_object().unwrap().clone());
        assert!(matches!(
            parse(value),
            Err(TransportError::InvalidMessage(_))
        ));
    }
}

#[test]
fn eof_empty_complete_and_partial_frames() {
    assert_eq!(read_message(&mut Cursor::new([])).unwrap(), None);
    let mut bytes = frame(&request());
    bytes.pop();
    let mut reader = Cursor::new(bytes);
    assert_eq!(read_message(&mut reader).unwrap(), Some(request()));
    assert_eq!(read_message(&mut reader).unwrap(), None);
    for bytes in [
        b"{\"version\":1".as_slice(),
        b"\"truncated",
        b"   ",
        b"\n",
        b"\r\n",
    ] {
        let mut reader = Cursor::new(bytes);
        assert!(matches!(
            read_message(&mut reader),
            Err(TransportError::InvalidJson(_))
        ));
        assert_eq!(read_message(&mut reader).unwrap(), None);
    }
}

#[test]
fn crlf_and_many_frames_work_across_buffer_boundaries() {
    let mut first = frame(&request());
    first.pop();
    first.extend_from_slice(b"\r\n");
    first.extend(frame(&request()));
    for capacity in [1, 2, 7, 4096] {
        let mut reader = BufReader::with_capacity(capacity, Cursor::new(first.clone()));
        assert_eq!(read_message(&mut reader).unwrap(), Some(request()));
        assert_eq!(read_message(&mut reader).unwrap(), Some(request()));
        assert_eq!(read_message(&mut reader).unwrap(), None);
    }
}

#[test]
fn malformed_json_invalid_utf8_and_consecutive_errors_do_not_desynchronize() {
    let mut bytes = b"nope\n\n{} {}\n{\"version\":1,}\n".to_vec();
    bytes.extend_from_slice(
        b"{\"version\":1,\"type\":\"event\",\"event\":\"\xff\",\"data\":null}\n",
    );
    bytes.extend_from_slice(b"{\"version\":2}\n{\"version\":1,\"type\":\"request\"}\n");
    bytes.extend(vec![b'x'; MAX_LINE_BYTES * 3]);
    bytes.push(b'\n');
    bytes.extend(frame(&request()));
    let mut reader = BufReader::with_capacity(3, Cursor::new(bytes));
    for _ in 0..5 {
        assert!(matches!(
            read_message(&mut reader),
            Err(TransportError::InvalidJson(_))
        ));
    }
    assert!(matches!(
        read_message(&mut reader),
        Err(TransportError::UnsupportedVersion)
    ));
    assert!(matches!(
        read_message(&mut reader),
        Err(TransportError::InvalidId)
    ));
    assert!(matches!(
        read_message(&mut reader),
        Err(TransportError::LineTooLong)
    ));
    assert_eq!(read_message(&mut reader).unwrap(), Some(request()));
}

fn sized_event(size: usize) -> Message {
    let base = Event::new("x", json!("")).into();
    let overhead = frame(&base).len() - 1;
    Event::new("x", json!("x".repeat(size - overhead))).into()
}

#[test]
fn exact_byte_limit_is_accepted_with_lf_crlf_and_eof() {
    for size in [MAX_LINE_BYTES - 1, MAX_LINE_BYTES] {
        let message = sized_event(size);
        let mut bytes = frame(&message);
        assert_eq!(bytes.len(), size + 1);
        bytes.pop();
        for delimiter in [b"".as_slice(), b"\n", b"\r\n"] {
            let mut line = bytes.clone();
            line.extend_from_slice(delimiter);
            let mut reader = BufReader::with_capacity(127, Cursor::new(line));
            assert_eq!(read_message(&mut reader).unwrap(), Some(message.clone()));
            assert_eq!(read_message(&mut reader).unwrap(), None);
        }
    }
}

#[test]
fn oversized_frames_are_drained_including_eof_and_repeated_oversize() {
    for size in [MAX_LINE_BYTES + 1, MAX_LINE_BYTES + 2, MAX_LINE_BYTES * 10] {
        for delimiter in [b"".as_slice(), b"\n", b"\r\n"] {
            let mut bytes = vec![b'x'; size];
            bytes.extend_from_slice(delimiter);
            let mut reader = BufReader::with_capacity(31, Cursor::new(bytes));
            assert!(matches!(
                read_message(&mut reader),
                Err(TransportError::LineTooLong)
            ));
            assert_eq!(read_message(&mut reader).unwrap(), None);
        }
    }
    let mut bytes = vec![b'x'; MAX_LINE_BYTES + 1];
    bytes.extend_from_slice(b"\n");
    bytes.extend(vec![b'x'; MAX_LINE_BYTES * 2]);
    bytes.extend_from_slice(b"\r\n");
    bytes.extend(frame(&request()));
    let mut reader = Cursor::new(bytes);
    for _ in 0..2 {
        assert!(matches!(
            read_message(&mut reader),
            Err(TransportError::LineTooLong)
        ));
    }
    assert_eq!(read_message(&mut reader).unwrap(), Some(request()));
    let mut bytes = frame(&sized_event(MAX_LINE_BYTES));
    bytes.pop();
    bytes.push(b'\r'); // 没有 LF 时 CR 属于载荷，需要计入长度。
    assert!(matches!(
        read_message(&mut Cursor::new(bytes)),
        Err(TransportError::LineTooLong)
    ));
}

#[test]
fn writer_rejects_oversized_result_error_and_escaped_utf8_without_writing() {
    let messages = [
        sized_event(MAX_LINE_BYTES + 1),
        Response::success("neo:1", json!("x".repeat(MAX_LINE_BYTES)))
            .unwrap()
            .into(),
        Response::failure("neo:1", ProtocolError::new("x", "x".repeat(MAX_LINE_BYTES)))
            .unwrap()
            .into(),
        Event::new("x", json!("\u{0000}".repeat(MAX_LINE_BYTES / 3))).into(),
        Event::new("x", json!("中".repeat(MAX_LINE_BYTES / 2))).into(),
    ];
    for message in messages {
        let mut bytes = b"unchanged".to_vec();
        assert!(matches!(
            write_message(&mut bytes, &message),
            Err(TransportError::LineTooLong)
        ));
        assert_eq!(bytes, b"unchanged");
    }
}

#[test]
fn writer_revalidates_mutated_messages_without_writing() {
    let Message::Request(mut bad_id) = request() else {
        unreachable!()
    };
    bad_id.id = "bad".into();
    let mut bad_version = Event::new("x", Value::Null);
    bad_version.version = 2;
    let mut bad_body = Response::success("neo:1", Value::Null).unwrap();
    bad_body.error = Some(ProtocolError::new("x", "x"));
    for message in [bad_id.into(), bad_version.into(), bad_body.into()] {
        let mut bytes = Vec::new();
        assert!(write_message(&mut bytes, &message).is_err());
        assert!(bytes.is_empty());
    }
}

#[test]
fn reader_retries_interrupted_and_propagates_io_errors() {
    struct InterruptedOnce {
        bytes: Cursor<Vec<u8>>,
        interrupted: bool,
    }
    impl Read for InterruptedOnce {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            self.bytes.read(out)
        }
    }
    impl BufRead for InterruptedOnce {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            self.bytes.fill_buf()
        }
        fn consume(&mut self, count: usize) {
            self.bytes.consume(count);
        }
    }
    let mut reader = InterruptedOnce {
        bytes: Cursor::new(frame(&request())),
        interrupted: false,
    };
    assert_eq!(read_message(&mut reader).unwrap(), Some(request()));
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }
    let error = read_message(&mut BufReader::new(Broken)).unwrap_err();
    assert!(matches!(error, TransportError::Io(_)));
    assert_eq!(error.code(), error_codes::IO_ERROR);
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn writer_handles_short_writes_interrupted_and_failure_without_flushing() {
    struct ShortWriter {
        bytes: Vec<u8>,
        interrupted: bool,
    }
    impl Write for ShortWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            self.bytes.push(bytes[0]);
            Ok(1)
        }
        fn flush(&mut self) -> io::Result<()> {
            panic!("caller owns flushing")
        }
    }
    let mut writer = ShortWriter {
        bytes: Vec::new(),
        interrupted: false,
    };
    write_message(&mut writer, &request()).unwrap();
    assert_eq!(writer.bytes, frame(&request()));
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    assert!(matches!(
        write_message(&mut Broken, &request()),
        Err(TransportError::Io(_))
    ));
    assert!(matches!(
        write_message(&mut &mut [0_u8; 0][..], &request()),
        Err(TransportError::Io(_))
    ));
}

// 固定种子的 xorshift，无外部随机依赖；分块计划可以稳定复现。
struct Chunks {
    bytes: Vec<u8>,
    position: usize,
    end: usize,
    seed: u64,
    max_chunk: usize,
}

impl Chunks {
    fn new(bytes: Vec<u8>, seed: u64, max_chunk: usize) -> Self {
        Self {
            bytes,
            position: 0,
            end: 0,
            seed,
            max_chunk,
        }
    }
}

impl Read for Chunks {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let buffer = self.fill_buf()?;
        let count = out.len().min(buffer.len());
        out[..count].copy_from_slice(&buffer[..count]);
        self.consume(count);
        Ok(count)
    }
}

impl BufRead for Chunks {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.position == self.end && self.end < self.bytes.len() {
            self.seed ^= self.seed << 13;
            self.seed ^= self.seed >> 7;
            self.seed ^= self.seed << 17;
            let count = 1 + (self.seed % self.max_chunk as u64) as usize;
            self.end = (self.position + count).min(self.bytes.len());
        }
        Ok(&self.bytes[self.position..self.end])
    }

    fn consume(&mut self, count: usize) {
        assert!(count <= self.end - self.position);
        self.position += count;
    }
}

#[test]
fn asynchronous_messages_survive_random_chunks_and_repeated_bad_frames() {
    let messages = [
        request(),
        Request::new("runtime:截图", "host.capture_region", json!({}))
            .unwrap()
            .into(),
        Event::new(
            "state_changed",
            json!({"text":"\u{7f}é中\u{1f600}\r\n\t\u{0000}"}),
        )
        .into(),
        Response::success("runtime:截图", json!({"job_id":"host:1"}))
            .unwrap()
            .into(),
        Response::success("neo:1", json!({"status":"pending"}))
            .unwrap()
            .into(),
        Event::new(
            "job.finished",
            json!({"job_id":"host:1","ok":true,"result":null}),
        )
        .into(),
        Response::failure("neo:2", ProtocolError::new("cancelled", "取消"))
            .unwrap()
            .into(),
    ];
    let mut bytes = Vec::new();
    let mut expected = Vec::new();
    for round in 0..8 {
        for bad in [b"\n".as_slice(), b"\r\n", b"{\n", b"{} {}\n", b"\xff\n"] {
            bytes.extend_from_slice(bad);
            expected.push(Err(error_codes::INVALID_JSON));
        }
        bytes.extend(std::iter::repeat_n(b'x', MAX_LINE_BYTES + round + 1));
        bytes.extend_from_slice(b"\r\n");
        expected.push(Err(error_codes::LINE_TOO_LONG));
        for (index, message) in messages.iter().enumerate() {
            bytes.extend(serde_json::to_vec(message).unwrap());
            bytes.extend_from_slice(if index % 2 == 0 { b"\r\n" } else { b"\n" });
            expected.push(Ok(message.clone()));
        }
    }
    bytes.extend(serde_json::to_vec(&messages[5]).unwrap());
    expected.push(Ok(messages[5].clone()));
    for seed in [1, 0x1234_5678, 0xdead_beef] {
        for max_chunk in [1, 2, 7, 127, MAX_LINE_BYTES * 2] {
            let mut reader = Chunks::new(bytes.clone(), seed, max_chunk);
            for item in &expected {
                match item {
                    Ok(message) => {
                        assert_eq!(read_message(&mut reader).unwrap(), Some(message.clone()))
                    }
                    Err(code) => assert_eq!(read_message(&mut reader).unwrap_err().code(), *code),
                }
            }
            assert_eq!(read_message(&mut reader).unwrap(), None);
            assert_eq!(reader.position, bytes.len());
        }
    }
}

#[test]
fn utf8_scalar_boundaries_and_controls_are_not_chunk_boundaries() {
    let scalars = "\u{0}\u{1}\u{8}\u{c}\n\r\t\u{1f}\u{7f}\u{80}\u{7ff}\u{800}\u{d7ff}\u{e000}\u{ffff}\u{10000}\u{10ffff}";
    let message: Message = Event::new(scalars, json!({scalars: scalars})).into();
    let bytes = frame(&message);
    for capacity in 1..=bytes.len() {
        let mut reader = BufReader::with_capacity(capacity, Cursor::new(&bytes));
        assert_eq!(read_message(&mut reader).unwrap(), Some(message.clone()));
    }
    for invalid in [
        b"\x80".as_slice(),
        b"\xc0\xaf",
        b"\xe0\x80\x80",
        b"\xed\xa0\x80",
        b"\xf4\x90\x80\x80",
        b"\xf0\x9f\x98",
        b"\x00",
        b"\x1f",
    ] {
        let mut bytes = b"{\"version\":1,\"type\":\"event\",\"event\":\"x\",\"data\":\"".to_vec();
        bytes.extend_from_slice(invalid);
        bytes.extend_from_slice(b"\"}\n");
        bytes.extend(frame(&request()));
        let mut reader = Chunks::new(bytes, 7, 1);
        assert_eq!(
            read_message(&mut reader).unwrap_err().code(),
            error_codes::INVALID_JSON
        );
        assert_eq!(read_message(&mut reader).unwrap(), Some(request()));
    }
}

#[test]
fn duplicate_protocol_fields_are_rejected_by_transport_and_serde() {
    for value in [
        serde_json::to_value(request()).unwrap(),
        serde_json::to_value(Response::success("neo:1", Value::Null).unwrap()).unwrap(),
        serde_json::to_value(Response::failure("neo:1", ProtocolError::new("x", "x")).unwrap())
            .unwrap(),
        serde_json::to_value(Event::new("x", Value::Null)).unwrap(),
    ] {
        let encoded = serde_json::to_string(&value).unwrap();
        for (key, field) in value.as_object().unwrap() {
            let duplicate = format!(
                "{{{}:{},{}",
                serde_json::to_string(key).unwrap(),
                field,
                &encoded[1..]
            );
            assert!(
                serde_json::from_str::<Message>(&duplicate).is_err(),
                "{duplicate}"
            );
            let mut bytes = duplicate.into_bytes();
            bytes.push(b'\n');
            bytes.extend(frame(&request()));
            let mut reader = Chunks::new(bytes, 123, 7);
            assert_eq!(
                read_message(&mut reader).unwrap_err().code(),
                error_codes::INVALID_MESSAGE
            );
            assert_eq!(read_message(&mut reader).unwrap(), Some(request()));
        }
    }
    for error in [
        r#"{"code":"a","code":"b","message":"x"}"#,
        r#"{"code":"a","message":"x","message":"y"}"#,
        r#"{"code":"a","message":"x","data":null,"data":{}}"#,
    ] {
        let text =
            format!(r#"{{"version":1,"type":"response","id":"neo:1","ok":false,"error":{error}}}"#);
        assert!(serde_json::from_str::<Message>(&text).is_err());
        assert!(serde_json::from_str::<Response>(&text).is_err());
        assert_eq!(
            read_message(&mut Cursor::new(text)).unwrap_err().code(),
            error_codes::INVALID_MESSAGE
        );
    }
    // 字段名转义后仍是同一字段，不能绕过重复检查。
    let text =
        r#"{"version":1,"type":"response","id":"neo:1","ok":false,"o\u006b":true,"result":null}"#;
    assert!(serde_json::from_str::<Message>(text).is_err());
    assert!(read_message(&mut Cursor::new(text)).is_err());
}

#[test]
fn unknown_fields_and_business_objects_keep_forward_compatibility() {
    for text in [
        r#"{"version":1,"type":"request","id":"neo:1","method":"future","params":{"x":1,"x":2},"future":1,"future":2}"#,
        r#"{"version":1,"type":"response","id":"neo:1","ok":true,"result":{"x":1,"x":2},"future":null,"future":{}}"#,
        r#"{"version":1,"type":"response","id":"neo:1","ok":false,"error":{"code":"x","message":"x","future":1,"future":2,"data":{"x":1,"x":2}}}"#,
        r#"{"version":1,"type":"event","event":"future","data":{"x":1,"x":2},"future":1,"future":2}"#,
        r#"{"version":1,"type":"event","event":"future","data":null,"ok":true,"ok":false}"#,
    ] {
        let direct: Message = serde_json::from_str(text).unwrap();
        let transport = read_message(&mut Cursor::new(text)).unwrap().unwrap();
        assert_eq!(direct, transport);
        assert_eq!(
            read_message(&mut Cursor::new(frame(&transport))).unwrap(),
            Some(transport)
        );
    }
    let text =
        r#"{"version":1,"type":"response","id":"neo:1","ok":true,"result":null,"error":null}"#;
    assert!(serde_json::from_str::<Message>(text).is_err());
    assert!(serde_json::from_str::<Response>(text).is_err());
}

#[test]
fn escaped_json_budget_counts_actual_bytes_at_limit() {
    for atom in [
        "x",
        "中",
        "\u{1f600}",
        "\u{0000}",
        "\n",
        "\"",
        "\\",
        "\t\u{1f}中\u{1f600}",
    ] {
        let base: Message = Event::new(atom, json!("")).into();
        let overhead = serde_json::to_vec(&base).unwrap().len();
        let atom_bytes = serde_json::to_vec(atom).unwrap().len() - 2;
        for target in [MAX_LINE_BYTES - 1, MAX_LINE_BYTES, MAX_LINE_BYTES + 1] {
            let budget = target - overhead;
            let text = atom.repeat(budget / atom_bytes) + &"x".repeat(budget % atom_bytes);
            let message: Message = Event::new(atom, json!(text)).into();
            let encoded = serde_json::to_vec(&message).unwrap();
            assert_eq!(encoded.len(), target);
            let mut output = Vec::new();
            let result = write_message(&mut output, &message);
            if target > MAX_LINE_BYTES {
                assert_eq!(result.unwrap_err().code(), error_codes::LINE_TOO_LONG);
                assert!(output.is_empty());
            } else {
                result.unwrap();
                assert_eq!(&output[..target], encoded);
                assert_eq!(output[target], b'\n');
            }
            for delimiter in [b"".as_slice(), b"\n", b"\r\n", b"\r"] {
                let mut bytes = encoded.clone();
                bytes.extend_from_slice(delimiter);
                let mut reader = Chunks::new(bytes, 42, 97);
                let actual_size = target + usize::from(delimiter == b"\r");
                if actual_size > MAX_LINE_BYTES {
                    assert_eq!(
                        read_message(&mut reader).unwrap_err().code(),
                        error_codes::LINE_TOO_LONG
                    );
                } else {
                    assert_eq!(read_message(&mut reader).unwrap(), Some(message.clone()));
                }
                assert_eq!(read_message(&mut reader).unwrap(), None);
            }
        }
    }
}

#[test]
fn partial_writes_never_report_success_even_when_only_lf_is_missing() {
    struct FailsAfter {
        bytes: Vec<u8>,
        limit: usize,
        zero: bool,
    }
    impl Write for FailsAfter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.bytes.len() == self.limit {
                return if self.zero {
                    Ok(0)
                } else {
                    Err(io::ErrorKind::BrokenPipe.into())
                };
            }
            let count = bytes.len().min(self.limit - self.bytes.len()).min(7);
            self.bytes.extend_from_slice(&bytes[..count]);
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            panic!("刷新由调用方负责")
        }
    }
    let message: Message = Response::success("neo:1", json!({"saved":true,"text":"中文\u{1f600}"}))
        .unwrap()
        .into();
    let full = frame(&message);
    for zero in [false, true] {
        for limit in 0..full.len() {
            let mut writer = FailsAfter {
                bytes: Vec::new(),
                limit,
                zero,
            };
            let error = write_message(&mut writer, &message).unwrap_err();
            let TransportError::Io(error) = error else {
                panic!("必须返回 I/O 错误")
            };
            assert_eq!(
                error.kind(),
                if zero {
                    io::ErrorKind::WriteZero
                } else {
                    io::ErrorKind::BrokenPipe
                }
            );
            assert_eq!(writer.bytes, full[..limit]);
            // EOF 可接受完整无换行 JSON，因此缺 LF 仍可能已被对端接收；写失败不保证业务未执行。
            if limit == full.len() - 1 {
                assert_eq!(
                    read_message(&mut Cursor::new(writer.bytes)).unwrap(),
                    Some(message.clone())
                );
            } else if limit == 0 {
                assert_eq!(read_message(&mut Cursor::new(writer.bytes)).unwrap(), None);
            } else {
                assert_eq!(
                    read_message(&mut Cursor::new(writer.bytes))
                        .unwrap_err()
                        .code(),
                    error_codes::INVALID_JSON
                );
            }
        }
    }
}

#[test]
fn underlying_errors_stop_immediately_mid_frame_and_while_draining() {
    struct FailingReader {
        remaining: usize,
        calls: usize,
        interrupts: usize,
        kind: io::ErrorKind,
    }
    impl Read for FailingReader {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("本测试直接使用 BufRead")
        }
    }
    impl BufRead for FailingReader {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            self.calls += 1;
            assert!(self.calls <= 4096, "错误后不应继续读取或死循环");
            if self.interrupts > 0 {
                self.interrupts -= 1;
                return Err(io::ErrorKind::Interrupted.into());
            }
            if self.remaining == 0 {
                return Err(self.kind.into());
            }
            // 不分配整条超长输入，模拟持续到来的无换行数据。
            Ok(&[b'x'; 8192][..self.remaining.min(8192)])
        }
        fn consume(&mut self, count: usize) {
            assert!(count <= self.remaining.min(8192));
            self.remaining -= count;
        }
    }
    for remaining in [0, 1, MAX_LINE_BYTES, MAX_LINE_BYTES * 128] {
        for kind in [
            io::ErrorKind::BrokenPipe,
            io::ErrorKind::WouldBlock,
            io::ErrorKind::UnexpectedEof,
        ] {
            let mut reader = FailingReader {
                remaining,
                calls: 0,
                interrupts: 3,
                kind,
            };
            let error = read_message(&mut reader).unwrap_err();
            let TransportError::Io(error) = error else {
                panic!("排空时也必须优先传播 I/O 错误")
            };
            assert_eq!(error.kind(), kind);
            assert_eq!(reader.remaining, 0);
            assert_eq!(reader.calls, remaining.div_ceil(8192) + 4);
        }
    }
}

#[test]
fn error_codes_are_stable_and_errors_have_display_messages() {
    for (error, code) in [
        (TransportError::UnsupportedVersion, "unsupported_version"),
        (TransportError::InvalidId, "invalid_id"),
        (TransportError::LineTooLong, "line_too_long"),
        (
            TransportError::InvalidMessage("x".into()),
            "invalid_message",
        ),
        (
            read_message(&mut Cursor::new(b"not json")).unwrap_err(),
            "invalid_json",
        ),
    ] {
        assert_eq!(error.code(), code);
        assert!(!error.to_string().is_empty());
    }
}
