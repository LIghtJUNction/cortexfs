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
                let receipt = DeliveryReceipt::new(
                    message.target.clone(),
                    format!("control-{}", message.target.conversation),
                );
                session.send_receipt(request_id, receipt)?;
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
                session.send_command_result(request_id, run, command_id, result)?;
            }
            ChannelFrameBody::Stop { .. } | ChannelFrameBody::Error { .. } => {
                return Err(ChannelControlError::Stopped);
            }
            _ => {}
        }
    }
}
