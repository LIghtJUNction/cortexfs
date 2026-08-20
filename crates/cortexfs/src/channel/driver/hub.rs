use std::{
    collections::BTreeMap,
    os::unix::net::UnixStream,
    sync::{Arc, Mutex, mpsc},
};

use cortexfs_channels::{
    ChannelActions, ChannelCapabilities, ChannelCommand, ChannelControlAction, ChannelFrame,
    ChannelFrameBody, ChannelId, DeliveryReceipt,
};

mod send;

#[cfg(test)]
mod tests;

type DriverWriter = Arc<Mutex<UnixStream>>;
#[derive(Clone, Debug)]
struct DriverPeer {
    writer: DriverWriter,
    capabilities: ChannelCapabilities,
    actions: ChannelActions,
}
type DriverWriters = Arc<Mutex<BTreeMap<String, DriverPeer>>>;
type PendingReceipts = Arc<Mutex<BTreeMap<String, mpsc::SyncSender<DeliveryReceipt>>>>;

/// Handle used by runtime code to send a message without an inbound trigger.
#[derive(Clone, Debug, Default)]
pub struct DriverHub {
    writers: DriverWriters,
    pending: PendingReceipts,
}

impl DriverHub {
    pub(crate) fn dispatch(
        &self,
        channel: &ChannelId,
        request_id: &str,
        action: ChannelControlAction,
    ) -> Result<(), super::DriverError> {
        let peer = self
            .writers
            .lock()
            .map_err(|_error| super::DriverError::Lock)?
            .get(channel.as_str())
            .cloned()
            .ok_or(super::DriverError::Unavailable)?;
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
                ChannelFrame::new(ChannelFrameBody::Command {
                    request_id: request_id.to_owned(),
                    session,
                    command_id,
                    command,
                    target,
                })
            }
            _ => return Err(super::DriverError::Rejected),
        };
        let mut stream = peer
            .writer
            .lock()
            .map_err(|_error| super::DriverError::Lock)?;
        super::write(&mut stream, &frame)
    }

    pub(super) fn attach(
        &self,
        channel: &ChannelId,
        writer: DriverWriter,
        capabilities: ChannelCapabilities,
        actions: ChannelActions,
    ) -> DriverRegistration {
        if let Ok(mut writers) = self.writers.lock() {
            writers.insert(
                channel.to_string(),
                DriverPeer {
                    writer: Arc::clone(&writer),
                    capabilities,
                    actions,
                },
            );
        }
        DriverRegistration {
            hub: self.clone(),
            channel: channel.clone(),
            writer,
        }
    }

    pub(super) fn detach(&self, channel: &ChannelId, writer: &DriverWriter) {
        if let Ok(mut writers) = self.writers.lock()
            && writers
                .get(channel.as_str())
                .is_some_and(|current| Arc::ptr_eq(&current.writer, writer))
        {
            writers.remove(channel.as_str());
        }
    }

    pub(crate) fn complete(&self, request_id: &str, receipt: DeliveryReceipt) -> bool {
        let Ok(mut pending) = self.pending.lock() else {
            return false;
        };
        let Some(sender) = pending.remove(request_id) else {
            return false;
        };
        sender.send(receipt).is_ok()
    }

    fn remove_pending(&self, request_id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            let _ignored = pending.remove(request_id);
        }
    }
}

pub(super) struct DriverRegistration {
    hub: DriverHub,
    channel: ChannelId,
    writer: DriverWriter,
}

impl Drop for DriverRegistration {
    fn drop(&mut self) {
        self.hub.detach(&self.channel, &self.writer);
    }
}
