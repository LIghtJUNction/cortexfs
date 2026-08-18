use std::net::TcpStream;

use cortexfs_runtime_client::RuntimeClientError;
use tungstenite::WebSocket;

use super::super::super::WebError;
use super::super::frame;
use super::{WorkerEvent, command};

pub(super) fn handle(
    socket: &mut WebSocket<TcpStream>,
    event: WorkerEvent,
    pending: &mut Option<command::PendingCommand>,
    request_id: &str,
    session: &str,
) -> Result<(), WebError> {
    match event {
        WorkerEvent::Event(event) => frame::send_event(socket, event)?,
        WorkerEvent::Command { event, reply } => {
            let (request_id, command_id) = command::command(&event)?;
            frame::send_event(socket, event)?;
            *pending = Some(command::PendingCommand::new(
                request_id,
                session.to_owned(),
                command_id,
                reply,
            ));
        }
        WorkerEvent::Error {
            request_id,
            message,
        } => command::send_error(socket, request_id, "EIO", message, true)?,
        WorkerEvent::Finished(result) => {
            if let Err(error) = result {
                command::send_error(
                    socket,
                    request_id.to_owned(),
                    "EIO",
                    safe_error(&error),
                    true,
                )?;
            }
        }
    }
    Ok(())
}

#[expect(
    clippy::pattern_type_mismatch,
    reason = "matching the borrowed error preserves the diagnostic without moving it"
)]
fn safe_error(error: &RuntimeClientError) -> &'static str {
    match error {
        RuntimeClientError::CannotConnect => "agent runtime is unavailable",
        RuntimeClientError::Rejected(_) => "agent runtime rejected the request",
        _ => "agent runtime request failed",
    }
}
