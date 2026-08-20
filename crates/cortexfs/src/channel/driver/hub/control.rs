use cortexfs_channels::{
    ChannelCommand, ChannelControlAction, ChannelFrame, ChannelFrameBody, ChannelId,
};

use super::super::{DriverError, write};
use super::DriverHub;

impl DriverHub {
    pub(crate) fn dispatch(
        &self,
        channel: &ChannelId,
        request_id: &str,
        action: ChannelControlAction,
    ) -> Result<(), DriverError> {
        let peer = self
            .writers
            .lock()
            .map_err(|_error| DriverError::Lock)?
            .get(channel.as_str())
            .cloned()
            .ok_or(DriverError::Unavailable)?;
        let mut wait = None;
        let frame = match action {
            ChannelControlAction::Send { message } if peer.capabilities.send => {
                ChannelFrame::new(ChannelFrameBody::Outbound {
                    request_id: request_id.to_owned(),
                    message,
                })
            }
            ChannelControlAction::Effect { target, effect }
                if peer.actions.supports(effect.action()) =>
            {
                ChannelFrame::new(ChannelFrameBody::Effect {
                    request_id: request_id.to_owned(),
                    target,
                    effect,
                })
            }
            ChannelControlAction::Command {
                session,
                command_id,
                command,
                target,
            } if peer.capabilities.commands
                || (peer.capabilities.tool_control
                    && matches!(&command, ChannelCommand::Invoke { .. })) =>
            {
                if matches!(&command, ChannelCommand::Invoke { .. }) {
                    wait = Some(self.register_command(request_id, &command_id)?);
                }
                ChannelFrame::new(ChannelFrameBody::Command {
                    request_id: request_id.to_owned(),
                    session,
                    command_id,
                    command,
                    target,
                })
            }
            _ => return Err(DriverError::Rejected),
        };
        let result = peer
            .writer
            .lock()
            .map_err(|_error| DriverError::Lock)
            .and_then(|mut stream| write(&mut stream, &frame));
        if result.is_err() {
            self.forget_command(request_id);
        }
        result?;
        wait.map_or(Ok(()), |receiver| self.wait_command(request_id, &receiver))
    }
}
