use cortexfs_channels::{
    ChannelCommandResult, ChannelError, ChannelFrameBody, ChannelHealth, HealthState,
    OutboundMessage,
};

use super::ChannelRuntime;
use crate::{ChannelSdkError, ChannelService};

impl<S: ChannelService> ChannelRuntime<S> {
    pub(super) fn dispatch(&mut self) -> Result<(), ChannelSdkError> {
        loop {
            match self.session.recv()? {
                ChannelFrameBody::Deliver { message, .. } => self.outbound(&message, None)?,
                ChannelFrameBody::Outbound {
                    request_id,
                    message,
                } => self.outbound(&message, Some(request_id))?,
                ChannelFrameBody::Effect {
                    request_id,
                    target,
                    effect,
                } => {
                    if let Err(error) = self.service.effect(&target, &effect) {
                        self.send_error(Some(request_id), &error)?;
                    }
                }
                ChannelFrameBody::Command {
                    request_id,
                    session,
                    command_id,
                    command,
                    target,
                } => {
                    let result = self
                        .service
                        .command(&session, &command_id, &command, target.as_ref())
                        .unwrap_or_else(|_error| ChannelCommandResult::Rejected {
                            reason: "channel operation failed".to_owned(),
                        });
                    self.session.send_frame(ChannelFrameBody::CommandResult {
                        request_id,
                        session,
                        command_id,
                        result,
                    })?;
                }
                ChannelFrameBody::HealthRequest { request_id } => {
                    let health = self
                        .service
                        .health()
                        .unwrap_or_else(|_error| ChannelHealth {
                            state: HealthState::Unavailable,
                            detail: Some("channel health check failed".to_owned()),
                        });
                    self.session
                        .send_frame(ChannelFrameBody::HealthResponse { request_id, health })?;
                }
                ChannelFrameBody::Event { event } => self
                    .service
                    .runtime_event(event)
                    .map_err(|error| ChannelSdkError::adapter("event", error))?,
                ChannelFrameBody::Stop { .. } => return Ok(()),
                ChannelFrameBody::Error { .. } => {
                    return Err(cortexfs_channels::ChannelDriverError::Protocol(
                        "channel runtime rejected a driver frame".to_owned(),
                    )
                    .into());
                }
                _ => {}
            }
        }
    }

    fn outbound(
        &mut self,
        message: &OutboundMessage,
        request_id: Option<String>,
    ) -> Result<(), ChannelSdkError> {
        match self.service.outbound(message) {
            Ok(receipt) => match request_id {
                Some(request_id) => self
                    .session
                    .send_receipt(request_id, receipt)
                    .map_err(Into::into),
                None => Ok(()),
            },
            Err(error) => self.send_error(request_id, &error),
        }
    }

    fn send_error(
        &self,
        request_id: Option<String>,
        error: &ChannelError,
    ) -> Result<(), ChannelSdkError> {
        let retryable = matches!(
            error,
            ChannelError::RateLimited { .. } | ChannelError::Transport(_)
        );
        self.session.send_frame(ChannelFrameBody::Error {
            request_id,
            code: "channel_operation_failed".to_owned(),
            message: "channel adapter operation failed".to_owned(),
            retryable,
        })?;
        Ok(())
    }
}
