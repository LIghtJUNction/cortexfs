#![expect(
    clippy::pattern_type_mismatch,
    reason = "channel command frames are matched by reference"
)]

use cortexfs_channels::{
    ChannelCommand, ChannelCommandResult, ChannelEffect, MessageTarget, OutboundMessage,
};
use reqwest::blocking::Client;

use super::super::{QqConfig, QqError};
use crate::channel::control::{ChannelControlError, ChannelControlHandler};

pub(super) struct Handler {
    client: Client,
    config: QqConfig,
}

impl Handler {
    pub(super) fn new(client: &Client, config: &QqConfig) -> Self {
        Self {
            client: client.clone(),
            config: config.clone(),
        }
    }
}

impl ChannelControlHandler for Handler {
    fn outbound(&mut self, message: &OutboundMessage) -> Result<(), ChannelControlError> {
        super::invoke::send(&self.client, &self.config, message).map_err(operation)
    }

    fn effect(
        &mut self,
        _target: &MessageTarget,
        _effect: &ChannelEffect,
    ) -> Result<(), ChannelControlError> {
        Err(operation(QqError::Protocol(
            "QQ does not expose live effects".to_owned(),
        )))
    }

    fn command(
        &mut self,
        _session: &str,
        _command_id: &str,
        command: &ChannelCommand,
        target: Option<&MessageTarget>,
    ) -> Result<ChannelCommandResult, ChannelControlError> {
        let Some(target) = target else {
            return Err(operation(QqError::Protocol("target is missing".to_owned())));
        };
        let ChannelCommand::Invoke { name, payload } = command else {
            return Err(operation(QqError::Protocol(
                "command is unsupported".to_owned(),
            )));
        };
        let payload = super::invoke::run(&self.client, &self.config, target, name, payload)
            .map_err(operation)?;
        Ok(ChannelCommandResult::Value { payload })
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the adapter error is converted into the generic control boundary"
)]
fn operation(error: QqError) -> ChannelControlError {
    ChannelControlError::Operation(error.to_string())
}
