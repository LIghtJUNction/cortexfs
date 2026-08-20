use std::{
    collections::BTreeMap,
    os::unix::net::UnixStream,
    sync::{Arc, Mutex, mpsc},
};

use cortexfs_channels::{
    ChannelActions, ChannelCapabilities, ChannelCommandResult, ChannelId, DeliveryReceipt,
};

mod control;
mod pending;
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
type PendingCommands =
    Arc<Mutex<BTreeMap<String, (String, mpsc::SyncSender<ChannelCommandResult>)>>>;

/// Handle used by runtime code to send a message without an inbound trigger.
#[derive(Clone, Debug, Default)]
pub struct DriverHub {
    writers: DriverWriters,
    pending: PendingReceipts,
    commands: PendingCommands,
}

impl DriverHub {
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
