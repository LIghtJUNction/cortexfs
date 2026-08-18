use std::net::TcpStream;
use std::sync::mpsc::SyncSender;

use cortexfs_runtime_client::interaction;
use tungstenite::WebSocket;

use super::super::super::WebError;
use super::super::frame;

pub(super) struct PendingCommand {
    request_id: String,
    session: String,
    command_id: String,
    reply: SyncSender<interaction::InteractionResult>,
}

impl PendingCommand {
    pub(super) fn new(
        request_id: String,
        session: String,
        command_id: String,
        reply: SyncSender<interaction::InteractionResult>,
    ) -> Self {
        Self {
            request_id,
            session,
            command_id,
            reply,
        }
    }

    pub(super) fn matches(&self, request_id: &str, session: &str, command_id: &str) -> bool {
        self.request_id == request_id && self.session == session && self.command_id == command_id
    }

    pub(super) fn reply(self) -> SyncSender<interaction::InteractionResult> {
        self.reply
    }

    pub(super) fn reject(self) {
        let _ignored = self.reply.send(interaction::InteractionResult::Rejected {
            reason: "websocket closed".to_owned(),
        });
    }
}

pub(super) fn submit(
    socket: &mut WebSocket<TcpStream>,
    request: interaction::InteractionRequest,
    pending: &mut Option<PendingCommand>,
    controls: &SyncSender<interaction::InteractionRequest>,
) -> Result<(), WebError> {
    if let interaction::InteractionRequest::CommandResult {
        request_id,
        session,
        command_id,
        result,
    } = request
    {
        let matches = pending
            .as_ref()
            .is_some_and(|pending| pending.matches(&request_id, &session, &command_id));
        if matches {
            if let Some(pending) = pending.take() {
                let _ignored = pending.reply().send(result);
            }
        } else {
            send_error(
                socket,
                request_id,
                "EINVAL",
                "unexpected command result",
                false,
            )?;
        }
        return Ok(());
    }
    let request_id = request.request_id().to_owned();
    if controls.try_send(request).is_err() {
        send_error(socket, request_id, "EBUSY", "control queue is full", true)?;
    }
    Ok(())
}

pub(super) fn send_error(
    socket: &mut WebSocket<TcpStream>,
    request_id: String,
    code: &'static str,
    message: &'static str,
    retryable: bool,
) -> Result<(), WebError> {
    frame::send_event(
        socket,
        interaction::InteractionEvent::Error {
            request_id,
            run: None,
            code: code.to_owned(),
            message: message.to_owned(),
            retryable,
        },
    )
}

#[expect(
    clippy::pattern_type_mismatch,
    reason = "matching the borrowed interaction event keeps the event ownership intact"
)]
pub(super) fn command(event: &interaction::InteractionEvent) -> Result<(String, String), WebError> {
    let interaction::InteractionEvent::Command {
        request_id,
        command_id,
        ..
    } = event
    else {
        return Err(WebError::InvalidFrame);
    };
    Ok((request_id.clone(), command_id.clone()))
}
