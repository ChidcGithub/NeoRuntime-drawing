//! 共享桌面应用；默认只显示帮助，绝不隐式启动图形界面。
mod editing;
mod features;
mod gui;
mod local_capture;
mod render_diagnostics;

use board_protocol::{Event, Message, read_message, write_message};
use board_session::{AppKind, Session};
use std::io;

pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppMode {
    Drawing,
    Blackboard,
}

impl AppMode {
    fn kind(self) -> AppKind {
        match self {
            Self::Drawing => AppKind::Drawing,
            Self::Blackboard => AppKind::Blackboard,
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Drawing => "Neo 画板",
            Self::Blackboard => "Neo 黑板",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Launch {
    Help,
    Version,
    Headless,
    Gui { hosted: bool },
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Launch> {
    let args: Vec<_> = args.into_iter().collect();
    if args.is_empty() || args == ["--help"] || args == ["-h"] {
        return Ok(Launch::Help);
    }
    if args == ["--version"] || args == ["-V"] {
        return Ok(Launch::Version);
    }
    let mut gui = false;
    let mut headless = false;
    let mut hosted = false;
    for arg in args {
        match arg.as_str() {
            "--gui" if !gui => gui = true,
            "--headless" if !headless => headless = true,
            "--hosted" if !hosted => hosted = true,
            _ => return Err(format!("未知或重复参数：{arg}；使用 --help 查看用法").into()),
        }
    }
    match (gui, headless, hosted) {
        (true, false, hosted) => Ok(Launch::Gui { hosted }),
        (false, true, false) => Ok(Launch::Headless),
        _ => Err("--gui 与 --headless 不能并用，--hosted 必须与 --gui 并用".into()),
    }
}

/// 解析进程命令行；无参数不会创建窗口。协议模式 stdout 仅输出 JSON Lines。
pub fn run(mode: AppMode) -> Result {
    match parse_args(std::env::args().skip(1))? {
        Launch::Help => {
            eprintln!(
                "{} {}\n\n  --gui                启动独立界面\n  --gui --hosted       由 Neo 宿主管理\n  --headless           无窗口 JSON Lines 模式\n  --version / -V       显示版本\n  --help / -h          显示帮助\n\n无参数仅显示帮助，不启动窗口。帮助和版本写入 stderr。\n不自动采集桌面或麦克风；Windows 独立模式点击截图可内置框选并上板（不依赖截图工具或剪贴板）。Agent 需宿主服务及逐次授权。\nCtrl+S 保存，Ctrl+Z 撤销，Ctrl+Y 重做，Esc 取消；未保存退出需确认。",
                mode.title(),
                env!("CARGO_PKG_VERSION")
            );
            Ok(())
        }
        Launch::Version => {
            eprintln!("{} {}", mode.title(), env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Launch::Headless => headless(mode),
        Launch::Gui { hosted } => gui::run(mode, hosted),
    }
}

pub(crate) fn emit(message: &Message) -> Result {
    write_message(&mut io::stdout().lock(), message)?;
    Ok(())
}

pub(crate) fn transport_event(error: &board_protocol::TransportError) -> Message {
    Event::new(
        "protocol_error",
        serde_json::json!({"code": error.code(), "message": error.to_string()}),
    )
    .into()
}

fn headless(mode: AppMode) -> Result {
    run_transport(
        &mut Session::new(mode.kind()),
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
        &recovery_directory(),
    )
}

pub(crate) fn recovery_directory() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("NeoRuntime-drawing")
        .join("recovery")
}

pub(crate) fn recover_document(
    session: &mut Session,
    directory: &std::path::Path,
) -> Result<Option<std::path::PathBuf>> {
    if !session.history.is_dirty(&session.document) || session.closed {
        return Ok(None);
    }
    std::fs::create_dir_all(directory)?;
    let path = directory.join(format!("recovery-{}.neoboard", board_core::new_id()));
    session.save_document(&path).map_err(|e| e.message)?;
    eprintln!("连接中断，未保存板书已写入恢复包：{}", path.display());
    Ok(Some(path))
}

/// 无窗口传输入口；恢复目录可注入，测试无需访问用户文档。
pub fn run_transport(
    session: &mut Session,
    input: &mut impl io::BufRead,
    output: &mut impl io::Write,
    recovery: &std::path::Path,
) -> Result {
    let result = (|| -> Result {
        write_message(output, &session.ready())?;
        loop {
            let messages = match read_message(input) {
                Ok(Some(Message::Request(request))) => session.handle(request),
                Ok(Some(Message::Response(response))) => session.handle_response(response),
                Ok(Some(Message::Event(event))) => session.handle_event(event),
                Ok(None) => {
                    for message in session.host_disconnected() {
                        write_message(output, &message)?;
                    }
                    break;
                }
                Err(error) => {
                    let fatal = matches!(error, board_protocol::TransportError::Io(_));
                    let mut messages = vec![transport_event(&error)];
                    // 丢帧可能正是任务完成事件；明确结束任务，不无限等待。
                    if fatal || session.state()["pending_jobs"].as_u64().unwrap_or(0) > 0 {
                        messages.extend(session.host_disconnected());
                    }
                    for message in messages {
                        write_message(output, &message)?;
                    }
                    if fatal {
                        return Err(error.into());
                    }
                    continue;
                }
            };
            for message in messages {
                write_message(output, &message)?;
            }
            if session.closed {
                break;
            }
        }
        Ok(())
    })();
    if result.is_err() {
        session.host_disconnected();
    }
    // stdout 已损坏时仍必须尝试持久化，不依赖协议发送成功。
    if let Err(error) = recover_document(session, recovery) {
        eprintln!("恢复包保存失败（目录 {}）：{error}", recovery.display());
        return Err(error);
    }
    result
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
