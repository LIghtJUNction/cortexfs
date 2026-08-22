use std::{
    io::Write,
    net::Shutdown,
    os::unix::net::UnixStream,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

use crate::{
    ChannelCommandResult, ChannelFrame, ChannelFrameBody, ChannelIncoming, ChannelIncomingEvent,
    DeliveryReceipt, InboundMessage,
};

use super::ChannelDriverError;

mod connect;
mod read;

/// Full-duplex client for a persistent channel-driver socket.
///
/// A reader thread keeps runtime-initiated frames available while the channel
/// is idle. The owner remains responsible for interpreting those frames.
#[derive(Clone, Debug)]
pub struct ChannelDriverSession {
    writer: Arc<Mutex<UnixStream>>,
    frames: Arc<Mutex<mpsc::Receiver<Result<ChannelFrameBody, ChannelDriverError>>>>,
}

impl ChannelDriverSession {
    /// Sends one frame to the runtime without taking ownership of the session.
    pub fn send_frame(&self, frame: ChannelFrameBody) -> Result<(), ChannelDriverError> {
        send(&self.writer, frame)
    }

    /// Sends an inbound event over the persistent session.
    pub fn send_inbound(&self, message: InboundMessage) -> Result<(), ChannelDriverError> {
        let event_id = message.id.clone();
        self.send_frame(ChannelFrameBody::Inbound { event_id, message })
    }

    /// Sends a provider-neutral non-message event over the persistent session.
    pub fn send_event(
        &self,
        event_id: String,
        event: ChannelIncomingEvent,
    ) -> Result<(), ChannelDriverError> {
        self.send_frame(ChannelFrameBody::InboundEvent { event_id, event })
    }

    /// Sends a message or event with a deterministic id when the item is an event.
    pub fn send_incoming(&self, incoming: ChannelIncoming) -> Result<(), ChannelDriverError> {
        let event_id = incoming.event_id();
        match incoming {
            ChannelIncoming::Message(message) => self.send_inbound(message),
            ChannelIncoming::Event(event) => self.send_event(event_id, event),
        }
    }

    /// Acknowledges a runtime-initiated outbound delivery.
    pub fn send_receipt(
        &self,
        request_id: String,
        receipt: DeliveryReceipt,
    ) -> Result<(), ChannelDriverError> {
        self.send_frame(ChannelFrameBody::Receipt {
            request_id,
            receipt,
        })
    }

    /// Returns one correlated command result to the runtime.
    pub fn send_command_result(
        &self,
        request_id: String,
        session: String,
        command_id: String,
        result: ChannelCommandResult,
    ) -> Result<(), ChannelDriverError> {
        self.send_frame(ChannelFrameBody::CommandResult {
            request_id,
            session,
            command_id,
            result,
        })
    }

    /// Requests a health response while keeping the session full-duplex.
    pub fn request_health(&self, request_id: String) -> Result<(), ChannelDriverError> {
        self.send_frame(ChannelFrameBody::HealthRequest { request_id })
    }

    /// Waits for the next frame, including unsolicited runtime frames.
    pub fn recv(&self) -> Result<ChannelFrameBody, ChannelDriverError> {
        self.frames
            .lock()
            .map_err(|_error| {
                ChannelDriverError::Protocol("channel reader lock poisoned".to_owned())
            })?
            .recv()
            .map_err(|_error| {
                ChannelDriverError::Protocol("channel driver reader stopped".to_owned())
            })?
    }

    /// Waits for the next frame for at most `timeout`.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<ChannelFrameBody, ChannelDriverError> {
        self.frames
            .lock()
            .map_err(|_error| {
                ChannelDriverError::Protocol("channel reader lock poisoned".to_owned())
            })?
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    ChannelDriverError::Protocol("channel driver frame timed out".to_owned())
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    ChannelDriverError::Protocol("channel driver reader stopped".to_owned())
                }
            })?
    }
}

impl Drop for ChannelDriverSession {
    fn drop(&mut self) {
        if Arc::strong_count(&self.writer) == 1
            && let Ok(writer) = self.writer.lock()
        {
            let _ignored = writer.shutdown(Shutdown::Both);
        }
    }
}

fn send(
    writer: &Arc<Mutex<UnixStream>>,
    frame: ChannelFrameBody,
) -> Result<(), ChannelDriverError> {
    let bytes = ChannelFrame::new(frame).encode()?;
    let mut stream = writer.lock().map_err(|_error| {
        ChannelDriverError::Protocol("channel writer lock poisoned".to_owned())
    })?;
    stream.write_all(&bytes)?;
    stream.flush().map_err(ChannelDriverError::Io)
}
