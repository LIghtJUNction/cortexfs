use std::{
    collections::BTreeMap,
    io::{BufRead, Write},
    sync::{Arc, Mutex},
};

use crate::{
    ChannelActions, ChannelCapabilities, ChannelCommand, ChannelCommandResult, ChannelError,
    ChannelHealth, ChannelId, ChannelService, ConversationId, DeliveryReceipt, InboundMessage,
    MessageBody, MessageTarget, OutboundMessage, Participant,
};
use cortexfs_channels::{ChannelFrame, ChannelFrameBody};

pub(super) struct Service {
    target: MessageTarget,
    calls: Arc<Mutex<u8>>,
}

impl Service {
    pub(super) fn new(target: MessageTarget) -> (Self, Arc<Mutex<u8>>) {
        let calls = Arc::new(Mutex::new(0));
        (
            Self {
                target,
                calls: Arc::clone(&calls),
            },
            calls,
        )
    }

    fn mark(&self) -> Result<(), ChannelError> {
        let mut calls = self.calls.lock().map_err(|_error| ChannelError::Closed)?;
        *calls = calls.saturating_add(1);
        drop(calls);
        Ok(())
    }
}

impl ChannelService for Service {
    fn id(&self) -> ChannelId {
        ChannelId::from_static("test")
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities::text()
    }

    fn actions(&self) -> ChannelActions {
        ChannelActions::empty()
    }

    fn start(&mut self) -> Result<(), ChannelError> {
        self.mark()
    }

    fn outbound(&mut self, _message: &OutboundMessage) -> Result<DeliveryReceipt, ChannelError> {
        self.mark()?;
        Ok(DeliveryReceipt {
            channel: self.id(),
            message_id: "remote-1".to_owned(),
            target: self.target.clone(),
            timestamp_ms: None,
        })
    }

    fn command(
        &mut self,
        _session: &str,
        _command_id: &str,
        _command: &ChannelCommand,
        _target: Option<&MessageTarget>,
    ) -> Result<ChannelCommandResult, ChannelError> {
        self.mark()?;
        Ok(ChannelCommandResult::Accepted)
    }

    fn health(&mut self) -> Result<ChannelHealth, ChannelError> {
        self.mark()?;
        Ok(ChannelHealth::ready())
    }

    fn stop(&mut self) -> Result<(), ChannelError> {
        self.mark()
    }
}

pub(super) fn target() -> Result<MessageTarget, ChannelError> {
    Ok(MessageTarget {
        channel: ChannelId::from_static("test"),
        conversation: ConversationId::new("conversation")?,
        thread: None,
        reply_to: None,
    })
}

pub(super) fn inbound() -> Result<InboundMessage, ChannelError> {
    Ok(InboundMessage {
        id: "event-1".to_owned(),
        target: target()?,
        sender: Participant::default(),
        body: MessageBody::text("hello")?,
        timestamp_ms: None,
        metadata: BTreeMap::new(),
    })
}

pub(super) fn read(reader: &mut impl BufRead) -> std::io::Result<ChannelFrameBody> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    ChannelFrame::decode(line.as_bytes())
        .map(|frame| frame.frame)
        .map_err(std::io::Error::other)
}

pub(super) fn write(writer: &mut impl Write, body: ChannelFrameBody) -> std::io::Result<()> {
    let bytes = ChannelFrame::new(body)
        .encode()
        .map_err(std::io::Error::other)?;
    writer.write_all(&bytes)
}
