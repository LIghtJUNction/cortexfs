#![expect(
    clippy::pattern_type_mismatch,
    reason = "channel command and effect frames are matched by reference"
)]

use cortexfs_channels::{
    ChannelCommand, ChannelCommandResult, ChannelEffect, MessageTarget, OutboundMessage,
};
use reqwest::blocking::Client;

use super::super::{DiscordConfig, DiscordError, api, effect, invoke, message};
use crate::channel::control::{ChannelControlError, ChannelControlHandler};

pub(super) struct Handler {
    client: Client,
    config: DiscordConfig,
}

impl Handler {
    pub(super) fn new(client: &Client, config: &DiscordConfig) -> Self {
        Self {
            client: client.clone(),
            config: config.clone(),
        }
    }
}

impl ChannelControlHandler for Handler {
    fn outbound(&mut self, message: &OutboundMessage) -> Result<(), ChannelControlError> {
        api::send_reply(
            &self.client,
            &self.config,
            message.target.conversation.as_str(),
            &message.body.text,
        )
        .map_err(operation)
    }

    fn effect(
        &mut self,
        target: &MessageTarget,
        effect: &ChannelEffect,
    ) -> Result<(), ChannelControlError> {
        apply_effect(&self.client, &self.config, target, effect).map_err(operation)
    }

    fn command(
        &mut self,
        _session: &str,
        command_id: &str,
        command: &ChannelCommand,
        target: Option<&MessageTarget>,
    ) -> Result<ChannelCommandResult, ChannelControlError> {
        let Some(target) = target else {
            return Err(operation(DiscordError::Invalid("target")));
        };
        let ChannelCommand::Invoke { name, payload } = command else {
            return Err(operation(DiscordError::Invalid("command")));
        };
        invoke::run(
            &self.client,
            &self.config,
            target,
            command_id,
            name,
            payload,
        )
        .map(|payload| ChannelCommandResult::Value { payload })
        .map_err(operation)
    }
}

fn apply_effect(
    client: &Client,
    config: &DiscordConfig,
    target: &MessageTarget,
    effect: &ChannelEffect,
) -> Result<(), DiscordError> {
    let channel = target.conversation.as_str();
    match effect {
        ChannelEffect::Typing { active: true } => effect::typing(client, config, channel),
        ChannelEffect::Typing { active: false } => Ok(()),
        ChannelEffect::Preview { text } => api::send_reply(client, config, channel, text),
        ChannelEffect::Reaction {
            message_id,
            emoji,
            remove,
        } => {
            if *remove {
                effect::remove(client, config, channel, message_id, emoji)
            } else {
                effect::react(client, config, channel, message_id, emoji)
            }
        }
        ChannelEffect::Edit { message_id, body } => {
            message::edit(client, config, channel, message_id, &body.text)
        }
        ChannelEffect::Delete { message_id } | ChannelEffect::Redact { message_id, .. } => {
            message::delete(client, config, channel, message_id)
        }
        ChannelEffect::Pin { message_id } => effect::pin(client, config, channel, message_id),
        ChannelEffect::Unpin { message_id } => effect::unpin(client, config, channel, message_id),
        ChannelEffect::MarkRead { .. } => Err(DiscordError::Invalid("mark_read")),
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the adapter error is converted into the generic control boundary"
)]
fn operation(error: DiscordError) -> ChannelControlError {
    ChannelControlError::Operation(error.to_string())
}
