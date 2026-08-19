use std::{sync::mpsc, time::Duration};

use cortexfs_channels::{
    ChannelFrame, ChannelFrameBody, ChannelId, DeliveryReceipt, OutboundMessage,
};

use super::super::{DriverError, write};
use super::DriverHub;

impl DriverHub {
    pub fn send(
        &self,
        channel: &ChannelId,
        request_id: String,
        message: OutboundMessage,
    ) -> Result<(), DriverError> {
        self.write_outbound(channel, request_id, message)
    }

    pub fn send_and_wait(
        &self,
        channel: &ChannelId,
        request_id: &str,
        message: OutboundMessage,
        timeout: Duration,
    ) -> Result<DeliveryReceipt, DriverError> {
        let (sender, receiver) = mpsc::sync_channel(1);
        self.pending
            .lock()
            .map_err(|_error| DriverError::Lock)?
            .insert(request_id.to_owned(), sender);
        if let Err(error) = self.write_outbound(channel, request_id.to_owned(), message) {
            self.remove_pending(request_id);
            return Err(error);
        }
        match receiver.recv_timeout(timeout) {
            Ok(receipt) => Ok(receipt),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.remove_pending(request_id);
                Err(DriverError::ReceiptTimeout)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(DriverError::Unavailable),
        }
    }

    fn write_outbound(
        &self,
        channel: &ChannelId,
        request_id: String,
        message: OutboundMessage,
    ) -> Result<(), DriverError> {
        let peer = self
            .writers
            .lock()
            .map_err(|_error| DriverError::Lock)?
            .get(channel.as_str())
            .cloned()
            .ok_or(DriverError::Unavailable)?;
        if !peer.capabilities.send {
            return Err(DriverError::Rejected);
        }
        let mut stream = peer.writer.lock().map_err(|_error| DriverError::Lock)?;
        write(
            &mut stream,
            &ChannelFrame::new(ChannelFrameBody::Outbound {
                request_id,
                message,
            }),
        )
    }
}
