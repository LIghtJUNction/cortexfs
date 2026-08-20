#![expect(
    clippy::pattern_type_mismatch,
    reason = "channel command and effect frames are matched by reference"
)]

use std::collections::BTreeMap;

use cortexfs_channels::{
    ChannelCodec, ChannelCommand, ChannelCommandResult, ChannelEffect, MessageBody, MessageTarget,
    OutboundMessage, platform::telegram::TelegramCodec,
};
use reqwest::blocking::Client;

use super::super::{TelegramConfig, TelegramError, api};
use crate::channel::control::{ChannelControlError, ChannelControlHandler};

pub(super) struct Handler {
    client: Client,
    config: TelegramConfig,
}

impl Handler {
    pub(super) fn new(client: &Client, config: &TelegramConfig) -> Self {
        Self {
            client: client.clone(),
            config: config.clone(),
        }
    }
}

impl ChannelControlHandler for Handler {
    fn outbound(&mut self, message: &OutboundMessage) -> Result<(), ChannelControlError> {
        let request = TelegramCodec
            .encode(message)
            .map_err(|error| control(TelegramError::Channel(error)))?;
        api::send_message(&self.client, &self.config, request).map_err(control)
    }

    fn effect(
        &mut self,
        target: &MessageTarget,
        effect: &ChannelEffect,
    ) -> Result<(), ChannelControlError> {
        if let ChannelEffect::Preview { text } = effect {
            let message = OutboundMessage {
                target: target.clone(),
                body: MessageBody::text(text.clone())
                    .map_err(|error| control(TelegramError::Channel(error)))?,
                metadata: BTreeMap::new(),
            };
            return self.outbound(&message);
        }
        if let Some(request) = TelegramCodec
            .encode_effect(target, effect)
            .map_err(|error| control(TelegramError::Channel(error)))?
        {
            api::send_message(&self.client, &self.config, request).map_err(control)?;
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
        let Some(target) = target else {
            return Err(control(TelegramError::Api("target is missing".to_owned())));
        };
        let ChannelCommand::Invoke { name, payload } = command else {
            return Err(control(TelegramError::Api(
                "command is unsupported".to_owned(),
            )));
        };
        super::invoke::run(&self.client, &self.config, target, name, payload)
            .map(|payload| ChannelCommandResult::Value { payload })
            .map_err(control)
    }
}
#[expect(
    clippy::needless_pass_by_value,
    reason = "the adapter error is converted into the generic control boundary"
)]
fn control(error: TelegramError) -> ChannelControlError {
    ChannelControlError::Operation(error.to_string())
}
