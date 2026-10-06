use crate::{Result, recover_document, transport_event};
use board_protocol::{Message, read_message, write_message};
use board_session::Session;
use std::{io, path::Path};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum StartupOutcome {
    Configured,
    Closed,
    Disconnected,
}

/// Hosted transport before any native window has ever been created.
/// The caller attaches the Session window adapter before entering this helper;
/// only Configured permits proceeding to native window creation.
pub(crate) fn run(
    session: &mut Session,
    input: &mut impl io::BufRead,
    output: &mut impl io::Write,
    recovery: &Path,
) -> Result<StartupOutcome> {
    let result = (|| -> Result<StartupOutcome> {
        if session.closed {
            return Ok(StartupOutcome::Closed);
        }
        if session.configured {
            return Ok(StartupOutcome::Configured);
        }
        write_message(output, &session.ready())?;
        while !session.configured && !session.closed {
            let mut accepted_close = false;
            let messages = match read_message(input) {
                Ok(Some(Message::Request(request))) => {
                    let closing =
                        request.method == "close" && session.state()["close_pending"] == false;
                    let messages = session.handle(request);
                    accepted_close = closing && session.state()["close_pending"] == true;
                    messages
                }
                Ok(Some(Message::Response(response))) => session.handle_response(response),
                Ok(Some(Message::Event(event))) => session.handle_event(event),
                Ok(None) => {
                    session.host_disconnected();
                    return Ok(StartupOutcome::Disconnected);
                }
                Err(error) => {
                    write_message(output, &transport_event(&error))?;
                    if matches!(error, board_protocol::TransportError::Io(_)) {
                        return Err(error.into());
                    }
                    continue;
                }
            };
            for message in messages {
                write_message(output, &message)?;
            }
            if accepted_close
                && !session.configured
                && let Some(request) = session.pending_window_request().cloned()
                && !request.visible
            {
                // No native windows have ever existed here. Confirm only this
                // accepted close, never attach/configure or capture completion.
                for message in session.acknowledge_window(&request.request_id, false) {
                    write_message(output, &message)?;
                }
            }
        }
        if session.closed {
            Ok(StartupOutcome::Closed)
        } else {
            Ok(StartupOutcome::Configured)
        }
    })();
    if result.is_err() {
        session.host_disconnected();
    }
    // Match the headless transport: broken stdout must not prevent recovery.
    if !matches!(result, Ok(StartupOutcome::Configured))
        && let Err(error) = recover_document(session, recovery)
    {
        eprintln!("恢复包保存失败（目录 {}）：{error}", recovery.display());
        return Err(error);
    }
    result
}

#[cfg(test)]
#[path = "hosted_startup_tests.rs"]
mod tests;
