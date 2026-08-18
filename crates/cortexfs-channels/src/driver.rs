use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
};

use thiserror::Error;

use crate::{
    ChannelCommandResult, ChannelEffect, ChannelFrame, ChannelFrameBody, ChannelHealth,
    ChannelIncoming, ChannelIncomingEvent, ChannelWireError, DeliveryReceipt, InboundMessage,
    OutboundMessage,
};

mod connect;
mod session;

pub use session::ChannelDriverSession;

/// Client for the process-isolated `cortexfs.channel.socket/v1` driver ABI.
#[derive(Debug)]
pub struct ChannelDriverClient {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
}

/// Stable failures at the Unix channel-driver boundary.
#[derive(Debug, Error)]
pub enum ChannelDriverError {
    #[error("channel driver I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("channel driver frame failed: {0}")]
    Frame(#[from] ChannelWireError),
    #[error("channel driver protocol failed: {0}")]
    Protocol(String),
}

impl ChannelDriverClient {
    /// Sends one inbound event and waits for its correlated final delivery.
    pub fn deliver(
        &mut self,
        message: InboundMessage,
    ) -> Result<OutboundMessage, ChannelDriverError> {
        self.deliver_with_command_handler(
            message,
            |_request_id, _session, _command_id, _command, _target| {
                Ok(ChannelCommandResult::Rejected {
                    reason: "channel driver has no interactive command reply path".to_owned(),
                })
            },
        )
    }

    /// Delivers one message and lets the adapter answer runtime commands.
    pub fn deliver_with_command_handler<F>(
        &mut self,
        message: InboundMessage,
        handler: F,
    ) -> Result<OutboundMessage, ChannelDriverError>
    where
        F: FnMut(
            &str,
            &str,
            &str,
            &crate::ChannelCommand,
            Option<&crate::MessageTarget>,
        ) -> Result<ChannelCommandResult, ChannelDriverError>,
    {
        self.deliver_with_handlers(message, handler, |_request_id, _message| {
            Err(ChannelDriverError::Protocol(
                "unsolicited outbound requires an outbound handler".to_owned(),
            ))
        })
    }

    /// Delivers a message while handling both runtime commands and proactive
    /// outbound messages on the same one-shot socket.
    pub fn deliver_with_handlers<F, G>(
        &mut self,
        message: InboundMessage,
        command_handler: F,
        outbound_handler: G,
    ) -> Result<OutboundMessage, ChannelDriverError>
    where
        F: FnMut(
            &str,
            &str,
            &str,
            &crate::ChannelCommand,
            Option<&crate::MessageTarget>,
        ) -> Result<ChannelCommandResult, ChannelDriverError>,
        G: FnMut(&str, &OutboundMessage) -> Result<DeliveryReceipt, ChannelDriverError>,
    {
        self.deliver_with_all_handlers(
            message,
            command_handler,
            outbound_handler,
            |_id, _target, _effect| Ok(()),
        )
    }

    /// Delivers one message while handling commands, effects, and proactive
    /// outbound messages on the same one-shot socket.
    pub fn deliver_with_all_handlers<F, G, H>(
        &mut self,
        message: InboundMessage,
        command_handler: F,
        outbound_handler: G,
        effect_handler: H,
    ) -> Result<OutboundMessage, ChannelDriverError>
    where
        F: FnMut(
            &str,
            &str,
            &str,
            &crate::ChannelCommand,
            Option<&crate::MessageTarget>,
        ) -> Result<ChannelCommandResult, ChannelDriverError>,
        G: FnMut(&str, &OutboundMessage) -> Result<DeliveryReceipt, ChannelDriverError>,
        H: FnMut(&str, &crate::MessageTarget, &ChannelEffect) -> Result<(), ChannelDriverError>,
    {
        let event_id = message.id.clone();
        self.deliver_frame(
            &event_id,
            ChannelFrameBody::Inbound {
                event_id: event_id.clone(),
                message,
            },
            command_handler,
            outbound_handler,
            effect_handler,
        )
    }

    /// Sends either a message or provider-neutral event and waits for its delivery.
    pub fn deliver_incoming(
        &mut self,
        incoming: ChannelIncoming,
    ) -> Result<OutboundMessage, ChannelDriverError> {
        self.deliver_incoming_with_command_handler(
            incoming,
            |_request_id, _session, _command_id, _command, _target| {
                Ok(ChannelCommandResult::Rejected {
                    reason: "channel driver has no interactive command reply path".to_owned(),
                })
            },
        )
    }

    /// Delivers a message or event with a provider-neutral command callback.
    pub fn deliver_incoming_with_command_handler<F>(
        &mut self,
        incoming: ChannelIncoming,
        handler: F,
    ) -> Result<OutboundMessage, ChannelDriverError>
    where
        F: FnMut(
            &str,
            &str,
            &str,
            &crate::ChannelCommand,
            Option<&crate::MessageTarget>,
        ) -> Result<ChannelCommandResult, ChannelDriverError>,
    {
        self.deliver_incoming_with_handlers(incoming, handler, |_request_id, _message| {
            Err(ChannelDriverError::Protocol(
                "unsolicited outbound requires an outbound handler".to_owned(),
            ))
        })
    }

    /// Delivers any incoming item while handling runtime commands and
    /// proactive outbound messages on the same one-shot socket.
    pub fn deliver_incoming_with_handlers<F, G>(
        &mut self,
        incoming: ChannelIncoming,
        command_handler: F,
        outbound_handler: G,
    ) -> Result<OutboundMessage, ChannelDriverError>
    where
        F: FnMut(
            &str,
            &str,
            &str,
            &crate::ChannelCommand,
            Option<&crate::MessageTarget>,
        ) -> Result<ChannelCommandResult, ChannelDriverError>,
        G: FnMut(&str, &OutboundMessage) -> Result<DeliveryReceipt, ChannelDriverError>,
    {
        self.deliver_incoming_with_all_handlers(
            incoming,
            command_handler,
            outbound_handler,
            |_id, _target, _effect| Ok(()),
        )
    }

    /// Delivers any incoming item while handling commands, effects, and
    /// proactive outbound messages on the same one-shot socket.
    pub fn deliver_incoming_with_all_handlers<F, G, H>(
        &mut self,
        incoming: ChannelIncoming,
        command_handler: F,
        outbound_handler: G,
        effect_handler: H,
    ) -> Result<OutboundMessage, ChannelDriverError>
    where
        F: FnMut(
            &str,
            &str,
            &str,
            &crate::ChannelCommand,
            Option<&crate::MessageTarget>,
        ) -> Result<ChannelCommandResult, ChannelDriverError>,
        G: FnMut(&str, &OutboundMessage) -> Result<DeliveryReceipt, ChannelDriverError>,
        H: FnMut(&str, &crate::MessageTarget, &ChannelEffect) -> Result<(), ChannelDriverError>,
    {
        let event_id = incoming.event_id();
        let frame = match incoming {
            ChannelIncoming::Message(message) => ChannelFrameBody::Inbound {
                event_id: event_id.clone(),
                message,
            },
            ChannelIncoming::Event(event) => ChannelFrameBody::InboundEvent {
                event_id: event_id.clone(),
                event,
            },
        };
        self.deliver_frame(
            &event_id,
            frame,
            command_handler,
            outbound_handler,
            effect_handler,
        )
    }

    fn deliver_frame<F, G, H>(
        &mut self,
        event_id: &str,
        frame: ChannelFrameBody,
        mut handler: F,
        mut outbound_handler: G,
        mut effect_handler: H,
    ) -> Result<OutboundMessage, ChannelDriverError>
    where
        F: FnMut(
            &str,
            &str,
            &str,
            &crate::ChannelCommand,
            Option<&crate::MessageTarget>,
        ) -> Result<ChannelCommandResult, ChannelDriverError>,
        G: FnMut(&str, &OutboundMessage) -> Result<DeliveryReceipt, ChannelDriverError>,
        H: FnMut(&str, &crate::MessageTarget, &ChannelEffect) -> Result<(), ChannelDriverError>,
    {
        self.send(frame)?;
        loop {
            match self.next_frame()? {
                ChannelFrameBody::Deliver {
                    request_id,
                    message,
                } if request_id == event_id => {
                    return Ok(message);
                }
                ChannelFrameBody::Command {
                    request_id,
                    session,
                    command_id,
                    command,
                    target,
                } => {
                    let result = handler(
                        &request_id,
                        &session,
                        &command_id,
                        &command,
                        target.as_ref(),
                    )?;
                    self.send(ChannelFrameBody::CommandResult {
                        request_id,
                        session,
                        command_id,
                        result,
                    })?;
                }
                ChannelFrameBody::Event {
                    event: crate::ChannelRuntimeEvent::Disconnected,
                } => {
                    return Err(ChannelDriverError::Protocol(
                        "driver disconnected".to_owned(),
                    ));
                }
                ChannelFrameBody::Error { .. } => {
                    return Err(ChannelDriverError::Protocol(
                        "runtime rejected the inbound event".to_owned(),
                    ));
                }
                ChannelFrameBody::Outbound {
                    request_id,
                    message,
                } => {
                    let receipt = outbound_handler(&request_id, &message)?;
                    self.send(ChannelFrameBody::Receipt {
                        request_id,
                        receipt,
                    })?;
                }
                ChannelFrameBody::Effect {
                    request_id,
                    target,
                    effect,
                } => effect_handler(&request_id, &target, &effect)?,
                _ => {}
            }
        }
    }

    /// Reads one independent frame from a persistent driver session.
    pub fn next_frame(&mut self) -> Result<ChannelFrameBody, ChannelDriverError> {
        let mut line = String::with_capacity(1024);
        if self.reader.read_line(&mut line)? == 0 {
            return Err(ChannelDriverError::Protocol(
                "driver closed before next frame".to_owned(),
            ));
        }
        Ok(ChannelFrame::decode(line.as_bytes())?.frame)
    }

    /// Acknowledges a runtime-initiated [`ChannelFrameBody::Outbound`] frame.
    pub fn send_receipt(
        &mut self,
        request_id: String,
        receipt: DeliveryReceipt,
    ) -> Result<(), ChannelDriverError> {
        self.send(ChannelFrameBody::Receipt {
            request_id,
            receipt,
        })
    }

    /// Sends a provider-neutral non-message event to the channel runtime.
    pub fn send_event(
        &mut self,
        event_id: String,
        event: ChannelIncomingEvent,
    ) -> Result<(), ChannelDriverError> {
        self.send(ChannelFrameBody::InboundEvent { event_id, event })
    }

    /// Requests the health of the connected runtime peer.
    pub fn health(&mut self, request_id: &str) -> Result<ChannelHealth, ChannelDriverError> {
        self.health_with_handlers(
            request_id,
            |_request_id, _session, _command_id, _command, _target| {
                Ok(ChannelCommandResult::Rejected {
                    reason: "channel driver has no interactive command reply path".to_owned(),
                })
            },
            |_request_id, _message| {
                Err(ChannelDriverError::Protocol(
                    "unsolicited outbound requires an outbound handler".to_owned(),
                ))
            },
            |_request_id, _target, _effect| Ok(()),
        )
    }

    /// Probes health while preserving full-duplex runtime frames.
    pub fn health_with_handlers<F, G, H>(
        &mut self,
        request_id: &str,
        mut command_handler: F,
        mut outbound_handler: G,
        mut effect_handler: H,
    ) -> Result<ChannelHealth, ChannelDriverError>
    where
        F: FnMut(
            &str,
            &str,
            &str,
            &crate::ChannelCommand,
            Option<&crate::MessageTarget>,
        ) -> Result<ChannelCommandResult, ChannelDriverError>,
        G: FnMut(&str, &OutboundMessage) -> Result<DeliveryReceipt, ChannelDriverError>,
        H: FnMut(&str, &crate::MessageTarget, &ChannelEffect) -> Result<(), ChannelDriverError>,
    {
        self.send(ChannelFrameBody::HealthRequest {
            request_id: request_id.to_owned(),
        })?;
        loop {
            match self.next_frame()? {
                ChannelFrameBody::HealthResponse {
                    request_id: response_id,
                    health,
                } if response_id == request_id => return Ok(health),
                ChannelFrameBody::Error {
                    request_id: Some(response_id),
                    message,
                    ..
                } if response_id == request_id => {
                    return Err(ChannelDriverError::Protocol(message));
                }
                ChannelFrameBody::Command {
                    request_id,
                    session,
                    command_id,
                    command,
                    target,
                } => {
                    let result = command_handler(
                        &request_id,
                        &session,
                        &command_id,
                        &command,
                        target.as_ref(),
                    )?;
                    self.send(ChannelFrameBody::CommandResult {
                        request_id,
                        session,
                        command_id,
                        result,
                    })?;
                }
                ChannelFrameBody::Outbound {
                    request_id,
                    message,
                } => {
                    let receipt = outbound_handler(&request_id, &message)?;
                    self.send(ChannelFrameBody::Receipt {
                        request_id,
                        receipt,
                    })?;
                }
                ChannelFrameBody::Effect {
                    request_id,
                    target,
                    effect,
                } => effect_handler(&request_id, &target, &effect)?,
                _ => {}
            }
        }
    }

    fn send(&mut self, frame: ChannelFrameBody) -> Result<(), ChannelDriverError> {
        self.writer.write_all(&ChannelFrame::new(frame).encode()?)?;
        self.writer.flush().map_err(ChannelDriverError::Io)
    }
}

#[cfg(test)]
mod tests;
