use std::{
    collections::BTreeMap,
    os::unix::net::UnixStream,
    sync::{Arc, Mutex, mpsc},
};

use cortexfs_channels::{ChannelId, DeliveryReceipt};

mod send;

#[cfg(test)]
mod tests;

type DriverWriter = Arc<Mutex<UnixStream>>;
type DriverWriters = Arc<Mutex<BTreeMap<String, DriverWriter>>>;
type PendingReceipts = Arc<Mutex<BTreeMap<String, mpsc::SyncSender<DeliveryReceipt>>>>;

/// Handle used by runtime code to send a message without an inbound trigger.
#[derive(Clone, Debug, Default)]
pub struct DriverHub {
    writers: DriverWriters,
    pending: PendingReceipts,
}

impl DriverHub {
    pub(super) fn attach(&self, channel: &ChannelId, writer: DriverWriter) -> DriverRegistration {
        if let Ok(mut writers) = self.writers.lock() {
            writers.insert(channel.to_string(), Arc::clone(&writer));
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
                .is_some_and(|current| Arc::ptr_eq(current, writer))
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
