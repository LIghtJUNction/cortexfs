#![expect(
    clippy::pattern_type_mismatch,
    reason = "channel command and effect frames are matched by reference"
)]

use cortexfs_channels::{
    ChannelCodec, ChannelCommand, ChannelCommandResult, ChannelEffect, MessageBody, MessageTarget,
    OutboundMessage, platform::mattermost::MattermostCodec,
};
use reqwest::blocking::Client;

use super::super::{MattermostConfig, MattermostError, api};
use super::invoke;
use crate::channel::control::{ChannelControlError, ChannelControlHandler};

pub(super) struct Handler {
    client: Client,
    config: MattermostConfig,
}

impl Handler {
    pub(super) fn new(client: &Client, config: &MattermostConfig) -> Self {
        Self {
            client: client.clone(),
            config: config.clone(),
        }
    }
}

impl ChannelControlHandler for Handler {
    fn outbound(&mut self, message: &OutboundMessage) -> Result<(), ChannelControlError> {
        let request = MattermostCodec.encode(message).map_err(channel)?;
        api::send(&self.client, &self.config, request).map_err(operation)
    }

    fn effect(
        &mut self,
        target: &MessageTarget,
        effect: &ChannelEffect,
    ) -> Result<(), ChannelControlError> {
        if let ChannelEffect::Preview { text } = effect {
            return self.outbound(&OutboundMessage {
                target: target.clone(),
                body: MessageBody::text(text.clone()).map_err(channel)?,
                metadata: std::collections::BTreeMap::new(),
            });
        }
        if let Some(request) = MattermostCodec
            .encode_effect(target, effect)
            .map_err(channel)?
        {
            api::send(&self.client, &self.config, request).map_err(operation)?;
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
            return Err(operation(MattermostError::Protocol(
                "command is unsupported".to_owned(),
            )));
        };
        invoke::run(&self.client, &self.config, target, name, payload)
            .map(|payload| ChannelCommandResult::Value { payload })
            .map_err(operation)
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the codec error is converted into the generic control boundary"
)]
fn channel(error: cortexfs_channels::ChannelError) -> ChannelControlError {
    ChannelControlError::Operation(error.to_string())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the adapter error is converted into the generic control boundary"
)]
fn operation(error: MattermostError) -> ChannelControlError {
    ChannelControlError::Operation(error.to_string())
}
