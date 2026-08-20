#![expect(
    clippy::pattern_type_mismatch,
    reason = "provider-neutral frames are matched by borrowed reference"
)]

use std::{collections::BTreeMap, fmt};

use cortexfs_channels::{
    ChannelCodec, ChannelCommand, ChannelCommandResult, ChannelEffect, MessageBody, MessageTarget,
    OutboundMessage, OutboundRequest,
};
use serde_json::Value;

use super::{ChannelControlError, ChannelControlHandler};

pub trait CodecTransport: Send {
    fn codec(&self) -> &dyn ChannelCodec;
    fn send(&mut self, request: OutboundRequest) -> Result<(), ChannelControlError>;
    fn invoke(
        &mut self,
        _target: Option<&MessageTarget>,
        _name: &str,
        _payload: &Value,
    ) -> Result<Value, ChannelControlError> {
        Err(ChannelControlError::Operation(
            "unsupported operation".to_owned(),
        ))
    }
}

pub struct CodecHandler {
    transport: Box<dyn CodecTransport>,
}

impl fmt::Debug for CodecHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodecHandler").finish_non_exhaustive()
    }
}

impl CodecHandler {
    #[must_use]
    pub fn new(transport: Box<dyn CodecTransport>) -> Self {
        Self { transport }
    }

    fn send_message(&mut self, message: &OutboundMessage) -> Result<(), ChannelControlError> {
        let request = self.transport.codec().encode(message).map_err(channel)?;
        self.transport.send(request)
    }
}

impl ChannelControlHandler for CodecHandler {
    fn outbound(&mut self, message: &OutboundMessage) -> Result<(), ChannelControlError> {
        self.send_message(message)
    }

    fn effect(
        &mut self,
        target: &MessageTarget,
        effect: &ChannelEffect,
    ) -> Result<(), ChannelControlError> {
        if let ChannelEffect::Preview { text } = effect {
            return self.send_message(&OutboundMessage {
                target: target.clone(),
                body: MessageBody::text(text.clone()).map_err(channel)?,
                metadata: BTreeMap::new(),
            });
        }
        if let Some(request) = self
            .transport
            .codec()
            .encode_effect(target, effect)
            .map_err(channel)?
        {
            self.transport.send(request)?;
        }
        Ok(())
    }

    fn command(
        &mut self,
        _session: &str,
        _command_id: &str,
        command: &ChannelCommand,
        target: Option<&MessageTarget>,
    ) -> Result<ChannelCommandResult, ChannelControlError> {
        let ChannelCommand::Invoke { name, payload } = command else {
            return Err(ChannelControlError::Operation(
                "unsupported command".to_owned(),
            ));
        };
        self.transport
            .invoke(target, name, payload)
            .map(|payload| ChannelCommandResult::Value { payload })
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the codec error is converted into the generic control boundary"
)]
fn channel(error: cortexfs_channels::ChannelError) -> ChannelControlError {
    ChannelControlError::Operation(error.to_string())
}
