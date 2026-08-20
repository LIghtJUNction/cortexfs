use cortexfs_channels::{
    ChannelCommandResult, ChannelDriverSession, ChannelFrameBody, DeliveryReceipt,
};

use super::{ChannelControlError, ChannelControlHandler};

#[expect(
    clippy::needless_pass_by_value,
    reason = "the session is moved into the dedicated control worker"
)]
pub(super) fn run(
    session: ChannelDriverSession,
    mut handler: Box<dyn ChannelControlHandler>,
) -> Result<(), ChannelControlError> {
    loop {
        match session.recv()? {
            ChannelFrameBody::Outbound {
                request_id,
                message,
            } => {
                handler.outbound(&message)?;
                session.send_receipt(request_id, receipt(&message))?;
            }
            ChannelFrameBody::Effect { target, effect, .. } => handler.effect(&target, &effect)?,
            ChannelFrameBody::Command {
                request_id,
                session: run,
                command_id,
                command,
                target,
            } => {
                let result = handler
                    .command(&run, &command_id, &command, target.as_ref())
                    .unwrap_or_else(|error| ChannelCommandResult::Rejected {
                        reason: error.to_string(),
                    });
                session.send_frame(ChannelFrameBody::CommandResult {
                    request_id,
                    session: run,
                    command_id,
                    result,
                })?;
            }
            ChannelFrameBody::Stop { .. } | ChannelFrameBody::Error { .. } => {
                return Err(ChannelControlError::Stopped);
            }
            _ => {}
        }
    }
}

fn receipt(message: &cortexfs_channels::OutboundMessage) -> DeliveryReceipt {
    DeliveryReceipt {
        channel: message.target.channel.clone(),
        message_id: format!("control-{}", message.target.conversation),
        target: message.target.clone(),
        timestamp_ms: None,
    }
}
