#![expect(
    clippy::pattern_type_mismatch,
    reason = "channel command and effect frames are matched by reference"
)]

use cortexfs_channels::{
    ChannelCodec, ChannelCommand, ChannelCommandResult, ChannelEffect, MessageBody, MessageTarget,
    OutboundMessage, platform::matrix::MatrixCodec,
};
use reqwest::blocking::Client;

use super::super::{MatrixConfig, MatrixError, api};
use super::invoke;
use crate::channel::control::{ChannelControlError, ChannelControlHandler};

pub(super) struct Handler {
    client: Client,
    config: MatrixConfig,
    transaction: u64,
}

impl Handler {
    pub(super) fn new(client: &Client, config: &MatrixConfig) -> Self {
        Self {
            client: client.clone(),
            config: config.clone(),
            transaction: 0,
        }
    }

    fn next_transaction(&mut self) -> String {
        self.transaction = self.transaction.saturating_add(1);
        format!("cortexfs-control-{}", self.transaction)
    }

    fn send_request(
        &mut self,
        request: cortexfs_channels::OutboundRequest,
    ) -> Result<(), ChannelControlError> {
        let transaction = self.next_transaction();
        api::send(&self.client, &self.config, request, &transaction).map_err(operation)
    }
}

impl ChannelControlHandler for Handler {
    fn outbound(&mut self, message: &OutboundMessage) -> Result<(), ChannelControlError> {
        self.send_request(MatrixCodec.encode(message).map_err(channel)?)
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
        if let Some(request) = MatrixCodec.encode_effect(target, effect).map_err(channel)? {
            self.send_request(request)?;
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
            return Err(operation(MatrixError::Protocol(
                "command is unsupported".to_owned(),
            )));
        };
        invoke::run(
            &self.client,
            &self.config,
            target,
            name,
            payload,
            &mut self.transaction,
        )
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
fn operation(error: MatrixError) -> ChannelControlError {
    ChannelControlError::Operation(error.to_string())
}
