//! Synchronous UTF-8 JSON Lines framing, independent of dispatchers and GUIs.
//! The 64 KiB limit excludes the LF or CRLF delimiter. A complete JSON value at
//! EOF is accepted without a final newline; an incomplete value is rejected.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::fmt;
use std::io::{self, BufRead, Write};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// Stable wire error codes. Applications may also supply their own codes.
pub mod error_codes {
    pub const INVALID_JSON: &str = "invalid_json";
    pub const INVALID_MESSAGE: &str = "invalid_message";
    pub const UNSUPPORTED_VERSION: &str = "unsupported_version";
    pub const INVALID_ID: &str = "invalid_id";
    pub const LINE_TOO_LONG: &str = "line_too_long";
    pub const IO_ERROR: &str = "io_error";
    pub const METHOD_NOT_FOUND: &str = "method_not_found";
    pub const INVALID_PARAMS: &str = "invalid_params";
    pub const INTERNAL_ERROR: &str = "internal_error";
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl ProtocolError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            data: None,
        }
    }
}

#[derive(Debug)]
pub enum TransportError {
    Io(io::Error),
    InvalidJson(serde_json::Error),
    UnsupportedVersion,
    InvalidId,
    InvalidMessage(String),
    LineTooLong,
}

impl TransportError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => error_codes::IO_ERROR,
            Self::InvalidJson(_) => error_codes::INVALID_JSON,
            Self::UnsupportedVersion => error_codes::UNSUPPORTED_VERSION,
            Self::InvalidId => error_codes::INVALID_ID,
            Self::InvalidMessage(_) => error_codes::INVALID_MESSAGE,
            Self::LineTooLong => error_codes::LINE_TOO_LONG,
        }
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "protocol I/O error: {error}"),
            Self::InvalidJson(error) => write!(f, "invalid JSON: {error}"),
            Self::UnsupportedVersion => write!(f, "version must be {PROTOCOL_VERSION}"),
            Self::InvalidId => write!(
                f,
                "id must start with neo: or runtime: and have a nonempty suffix without whitespace or control characters"
            ),
            Self::InvalidMessage(error) => write!(f, "invalid message: {error}"),
            Self::LineTooLong => write!(f, "JSON line exceeds {MAX_LINE_BYTES} bytes"),
        }
    }
}

impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidJson(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for TransportError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum RequestType {
    #[serde(rename = "request")]
    Request,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum ResponseType {
    #[serde(rename = "response")]
    Response,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum EventType {
    #[serde(rename = "event")]
    Event,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub version: u32,
    #[serde(rename = "type")]
    kind: RequestType,
    pub id: String,
    pub method: String,
    #[serde(deserialize_with = "Value::deserialize")]
    pub params: Value,
}

impl Request {
    pub fn new(
        id: impl Into<String>,
        method: impl Into<String>,
        params: Value,
    ) -> Result<Self, TransportError> {
        let id = id.into();
        validate_id(&id)?;
        Ok(Self {
            version: PROTOCOL_VERSION,
            kind: RequestType::Request,
            id,
            method: method.into(),
            params,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub version: u32,
    #[serde(rename = "type")]
    kind: ResponseType,
    pub id: String,
    pub ok: bool,
    #[serde(
        default,
        deserialize_with = "present_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub result: Option<Value>,
    #[serde(
        default,
        deserialize_with = "present_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub error: Option<ProtocolError>,
}

// 保留显式 result:null；error 出现时必须是错误对象，不能用 null 充当缺省。
fn present_value<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

fn present_error<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ProtocolError>, D::Error> {
    ProtocolError::deserialize(deserializer).map(Some)
}

impl Response {
    pub fn success(id: impl Into<String>, result: Value) -> Result<Self, TransportError> {
        let id = id.into();
        validate_id(&id)?;
        Ok(Self {
            version: PROTOCOL_VERSION,
            kind: ResponseType::Response,
            id,
            ok: true,
            result: Some(result),
            error: None,
        })
    }

    pub fn failure(id: impl Into<String>, error: ProtocolError) -> Result<Self, TransportError> {
        let id = id.into();
        validate_id(&id)?;
        Ok(Self {
            version: PROTOCOL_VERSION,
            kind: ResponseType::Response,
            id,
            ok: false,
            result: None,
            error: Some(error),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub version: u32,
    #[serde(rename = "type")]
    kind: EventType,
    pub event: String,
    #[serde(deserialize_with = "Value::deserialize")]
    pub data: Value,
}

impl Event {
    pub fn new(event: impl Into<String>, data: Value) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            kind: EventType::Event,
            event: event.into(),
            data,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Message {
    Request(Request),
    Response(Response),
    Event(Event),
}

impl From<Request> for Message {
    fn from(value: Request) -> Self {
        Self::Request(value)
    }
}
impl From<Response> for Message {
    fn from(value: Response) -> Self {
        Self::Response(value)
    }
}
impl From<Event> for Message {
    fn from(value: Event) -> Self {
        Self::Event(value)
    }
}

impl<'de> Deserialize<'de> for Message {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // 直接反序列化协议结构，避免 Value 合并重复字段后掩盖歧义。
        // 业务载荷仍用 Value，未知字段继续由 serde 忽略。
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum WireMessage {
            Request(Request),
            Response(Response),
            Event(Event),
        }
        let message = match WireMessage::deserialize(deserializer)? {
            WireMessage::Request(request) => Self::Request(request),
            WireMessage::Response(response) => Self::Response(response),
            WireMessage::Event(event) => Self::Event(event),
        };
        message.validate().map_err(serde::de::Error::custom)?;
        Ok(message)
    }
}

fn validate_id(id: &str) -> Result<(), TransportError> {
    let suffix = id
        .strip_prefix("neo:")
        .or_else(|| id.strip_prefix("runtime:"));
    if suffix.is_none_or(str::is_empty) || id.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(TransportError::InvalidId);
    }
    Ok(())
}

impl Message {
    pub fn validate(&self) -> Result<(), TransportError> {
        let version = match self {
            Self::Request(request) => request.version,
            Self::Response(response) => response.version,
            Self::Event(event) => event.version,
        };
        if version != PROTOCOL_VERSION {
            return Err(TransportError::UnsupportedVersion);
        }
        match self {
            Self::Request(request) => validate_id(&request.id)?,
            Self::Response(response) => {
                validate_id(&response.id)?;
                if response.ok != response.result.is_some()
                    || response.ok == response.error.is_some()
                {
                    return Err(TransportError::InvalidMessage(
                        "success requires only result; failure requires only error".into(),
                    ));
                }
            }
            Self::Event(_) => {}
        }
        Ok(())
    }
}

fn decode_value(value: Value, line: &[u8]) -> Result<Message, TransportError> {
    let object = value
        .as_object()
        .ok_or_else(|| TransportError::InvalidMessage("expected an object".into()))?;
    if object.get("version").and_then(Value::as_u64) != Some(u64::from(PROTOCOL_VERSION)) {
        return Err(TransportError::UnsupportedVersion);
    }
    let kind = object.get("type").and_then(Value::as_str);
    if matches!(kind, Some("request" | "response")) {
        validate_id(
            object
                .get("id")
                .and_then(Value::as_str)
                .ok_or(TransportError::InvalidId)?,
        )?;
    }
    // 字段是否存在有语义：result:null 是结果，但 error:null 不是错误对象。
    if kind == Some("response") {
        let valid = match object.get("ok").and_then(Value::as_bool) {
            Some(true) => object.contains_key("result") && !object.contains_key("error"),
            Some(false) => {
                !object.contains_key("result") && object.get("error").is_some_and(Value::is_object)
            }
            None => false,
        };
        if !valid {
            return Err(TransportError::InvalidMessage(
                "response must contain exactly its matching result or error".into(),
            ));
        }
    }
    // 仍从原始字节构造结构体，让 serde 拒绝重复的协议字段。
    let message = match kind {
        Some("request") => serde_json::from_slice(line).map(Message::Request),
        Some("response") => serde_json::from_slice(line).map(Message::Response),
        Some("event") => serde_json::from_slice(line).map(Message::Event),
        _ => {
            return Err(TransportError::InvalidMessage(
                "unknown or missing message type".into(),
            ));
        }
    }
    .map_err(|error| TransportError::InvalidMessage(error.to_string()))?;
    message.validate()?;
    Ok(message)
}

/// 读取一帧，消费损坏或超长的整行以便下一次调用恢复。
/// 分帧缓冲最多 MAX_LINE_BYTES + 1 字节，与行长无关（BufRead 缓冲由调用方持有）。
/// 仅在 EOF 返回 None；Interrupted 会重试，其他 I/O 错误须由调用方终止传输。
pub fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<Message>, TransportError> {
    let mut line = Vec::with_capacity(MAX_LINE_BYTES + 1);
    let mut oversized = false;
    let mut terminated = false;
    loop {
        let buffer = match reader.fill_buf() {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if buffer.is_empty() {
            if oversized {
                return Err(TransportError::LineTooLong);
            }
            if line.is_empty() {
                return Ok(None);
            }
            break;
        }
        let newline = buffer.iter().position(|byte| *byte == b'\n');
        let count = newline.unwrap_or(buffer.len());
        if !oversized {
            if count > MAX_LINE_BYTES + 1 - line.len() {
                oversized = true;
            } else {
                line.extend_from_slice(&buffer[..count]);
            }
        }
        reader.consume(count + usize::from(newline.is_some()));
        if newline.is_some() {
            terminated = true;
            break;
        }
    }
    if terminated && line.last() == Some(&b'\r') {
        line.pop();
    }
    if oversized || line.len() > MAX_LINE_BYTES {
        return Err(TransportError::LineTooLong);
    }
    let value = serde_json::from_slice(&line).map_err(TransportError::InvalidJson)?;
    decode_value(value, &line).map(Some)
}

struct LimitedBuffer {
    bytes: Vec<u8>,
    exceeded: bool,
}

impl Write for LimitedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_LINE_BYTES - self.bytes.len() {
            self.exceeded = true;
            return Err(io::Error::other("JSON line is too long"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// 先校验并序列化到有界缓冲，再写入目标；无效或超长消息不会写出任何字节。
/// I/O 失败可能已经写出部分帧，调用方必须停止使用该流，不可当作业务成功或安全重试。
/// 完整 JSON 即使缺 LF 也可能被对端按 EOF 接收；刷新及其错误由调用方负责。
pub fn write_message<W: Write>(writer: &mut W, message: &Message) -> Result<(), TransportError> {
    message.validate()?;
    let mut buffer = LimitedBuffer {
        bytes: Vec::with_capacity(MAX_LINE_BYTES + 1),
        exceeded: false,
    };
    if let Err(error) = serde_json::to_writer(&mut buffer, message) {
        return Err(if buffer.exceeded {
            TransportError::LineTooLong
        } else {
            TransportError::InvalidJson(error)
        });
    }
    buffer.bytes.push(b'\n');
    writer.write_all(&buffer.bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests;
