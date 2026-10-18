//! 宿主日志收集接入（Neo docs-pri/board-log-collection.md）：仅在协议会话
//! （`--gui --hosted` / `--headless`）中输出 `log` 事件，走与 RPC 相同的
//! stdout 帧通道；stderr 不写日志。只允许安全概括——错误类别、计数、耗时、
//! 状态名；路径、命令、原始错误文本与内容一律不入日志。
//!
//! 客户端约束（与宿主侧一致并更严）：每秒最多 64 条，超出静默丢弃并计数；
//! 消息单行化并裁剪；fields ≤16 项、key 非空 ≤64 字节、值为字符串 ≤256 字节。
//! 时间戳、线程名、组件名由宿主补齐，子进程不上报。

use board_protocol::{Event, Message};
use serde_json::json;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub(crate) const RATE_LIMIT_PER_SECOND: u32 = 64;
const WINDOW: Duration = Duration::from_secs(1);
const MAX_MESSAGE_BYTES: usize = 512;
const MAX_FIELD_KEY_BYTES: usize = 64;
const MAX_FIELD_VALUE_BYTES: usize = 256;
const MAX_FIELDS: usize = 16;
const EVENT_NAME: &str = "log";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// 收集器：`info/warn/...` 入队（限速 + 裁剪 + 单行化），`drain()` 由传输层在
/// 每个协议循环后写出到同一 stdout 帧通道。`enabled = false`（独立模式）时
/// 直接丢弃，零开销。
pub(crate) struct BoardLog {
    enabled: bool,
    window_start: Instant,
    sent_in_window: u32,
    dropped: u64,
    pending: VecDeque<Message>,
    clock: Box<dyn Fn() -> Instant>,
}

impl BoardLog {
    pub(crate) fn new(enabled: bool) -> Self {
        Self::with_clock(enabled, Box::new(Instant::now))
    }

    fn with_clock(enabled: bool, clock: Box<dyn Fn() -> Instant>) -> Self {
        Self {
            enabled,
            window_start: clock(),
            sent_in_window: 0,
            dropped: 0,
            pending: VecDeque::new(),
            clock,
        }
    }

    pub(crate) fn debug(&mut self, message: impl AsRef<str>, fields: &[(&str, &str)]) {
        self.log(Level::Debug, message, fields);
    }

    pub(crate) fn info(&mut self, message: impl AsRef<str>, fields: &[(&str, &str)]) {
        self.log(Level::Info, message, fields);
    }

    pub(crate) fn warn(&mut self, message: impl AsRef<str>, fields: &[(&str, &str)]) {
        self.log(Level::Warn, message, fields);
    }

    pub(crate) fn error(&mut self, message: impl AsRef<str>, fields: &[(&str, &str)]) {
        self.log(Level::Error, message, fields);
    }

    pub(crate) fn log(&mut self, level: Level, message: impl AsRef<str>, fields: &[(&str, &str)]) {
        if !self.enabled {
            return;
        }
        let now = (self.clock)();
        if now.duration_since(self.window_start) >= WINDOW {
            self.window_start = now;
            self.sent_in_window = 0;
        }
        if self.sent_in_window >= RATE_LIMIT_PER_SECOND {
            self.dropped += 1;
            return;
        }
        self.sent_in_window += 1;
        let message = clip_single_line(message.as_ref(), MAX_MESSAGE_BYTES);
        if message.is_empty() {
            return;
        }
        let mut field_map = serde_json::Map::new();
        for (key, value) in fields.iter().take(MAX_FIELDS) {
            let key = clip_single_line(key, MAX_FIELD_KEY_BYTES);
            if key.is_empty() || field_map.contains_key(&key) {
                continue;
            }
            field_map.insert(key, json!(clip_single_line(value, MAX_FIELD_VALUE_BYTES)));
        }
        let data = if field_map.is_empty() {
            json!({ "level": level.as_str(), "message": message })
        } else {
            json!({ "level": level.as_str(), "message": message, "fields": field_map })
        };
        self.pending
            .push_back(Message::Event(Event::new(EVENT_NAME, data)));
    }

    /// 取出全部待发送事件（FIFO）；由调用方写到当前传输的 stdout 帧通道。
    pub(crate) fn drain(&mut self) -> Vec<Message> {
        self.pending.drain(..).collect()
    }

    /// 因限速被静默丢弃的条数（诊断用，与宿主侧计数互补）。
    pub(crate) fn dropped(&self) -> u64 {
        self.dropped
    }
}

/// 单行化 + 按字节裁剪（保持 UTF-8 边界），截断时追加省略号。
fn clip_single_line(text: &str, max_bytes: usize) -> String {
    let single_line = text.replace('\r', " ").replace('\n', " ");
    let trimmed = single_line.trim();
    if trimmed.len() <= max_bytes {
        return trimmed.to_owned();
    }
    let mut end = max_bytes.saturating_sub("…".len());
    while end > 0 && !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &trimmed[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: u64) -> Box<dyn Fn() -> Instant> {
        Box::new(move || Instant::now() + Duration::from_secs(seconds))
    }

    fn level_of(message: &Message) -> String {
        let Message::Event(event) = message else {
            panic!("expected event")
        };
        assert_eq!(event.event, "log");
        event.data["level"].as_str().unwrap().to_owned()
    }

    #[test]
    fn disabled_collector_never_queues() {
        let mut logger = BoardLog::new(false);
        logger.info("ignored", &[("k", "v")]);
        assert!(logger.drain().is_empty());
        assert_eq!(logger.dropped(), 0);
    }

    #[test]
    fn event_payload_shape_and_field_rules() {
        let mut logger = BoardLog::new(true);
        logger.info(
            "recognition failed\nsecond line",
            &[
                ("backend", "texeller"),
                ("kind", "unsupported"),
                ("", "empty key dropped"),
                ("value", "ok"),
            ],
        );
        let messages = logger.drain();
        assert_eq!(messages.len(), 1);
        assert_eq!(level_of(&messages[0]), "info");
        let Message::Event(event) = &messages[0] else {
            panic!()
        };
        assert!(event.data["message"].as_str().unwrap().contains(' '));
        assert!(!event.data["message"].as_str().unwrap().contains('\n'));
        let fields = event.data["fields"].as_object().unwrap();
        assert_eq!(fields.len(), 3, "empty key dropped");
        assert_eq!(fields["backend"], "texeller");
        assert!(fields["backend"].is_string(), "values must be strings");
    }

    #[test]
    fn rate_limit_drops_silently_and_counts() {
        let mut logger = BoardLog::with_clock(true, at(0));
        for _ in 0..70 {
            logger.info("flood", &[]);
        }
        assert_eq!(logger.drain().len(), 64);
        assert_eq!(logger.dropped(), 6);
        // 新窗口恢复预算。
        let mut clocked = BoardLog::with_clock(true, at(0));
        for _ in 0..64 {
            clocked.info("flood", &[]);
        }
        assert_eq!(clocked.drain().len(), 64);
        assert_eq!(clocked.dropped(), 0);
        let _ = clocked.dropped();
    }

    #[test]
    fn long_message_is_clipped_to_single_line() {
        let mut logger = BoardLog::new(true);
        let long = "x".repeat(600) + "\n" + &"y".repeat(30);
        logger.warn(long, &[]);
        let message = logger.drain().remove(0);
        let Message::Event(event) = &message else {
            panic!()
        };
        let text = event.data["message"].as_str().unwrap();
        assert!(text.len() <= MAX_MESSAGE_BYTES);
        assert!(text.ends_with('…'));
        assert!(!text.contains('\n'));
    }

    #[test]
    fn fields_are_capped_and_values_clipped() {
        let mut logger = BoardLog::new(true);
        let fields: Vec<(String, String)> = (0..20)
            .map(|i| (format!("k{i}"), format!("v{}", "x".repeat(300))))
            .collect();
        let refs: Vec<(&str, &str)> = fields
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        logger.warn("capped", &refs);
        let message = logger.drain().remove(0);
        let Message::Event(event) = &message else {
            panic!()
        };
        let map = event.data["fields"].as_object().unwrap();
        assert_eq!(map.len(), MAX_FIELDS);
        assert!(
            map.values()
                .all(|v| v.as_str().unwrap().len() <= MAX_FIELD_VALUE_BYTES)
        );
    }
}
