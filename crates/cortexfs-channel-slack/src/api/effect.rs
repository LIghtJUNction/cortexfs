#![expect(
    clippy::redundant_pub_crate,
    reason = "effect helper is private driver plumbing"
)]

use cortexfs_channels::{ChannelEffect, ChannelError, MessageTarget};
use reqwest::Client;
use serde_json::json;

use crate::{config::Config, error::Result};

pub(crate) async fn apply(
    client: &Client,
    config: &Config,
    target: &MessageTarget,
    effect: ChannelEffect,
) -> Result<()> {
    let (path, body) = match effect {
        ChannelEffect::Typing { .. } | ChannelEffect::Preview { .. } => return Ok(()),
        ChannelEffect::Reaction {
            message_id,
            emoji,
            remove,
        } => (
            if remove {
                "reactions.remove"
            } else {
                "reactions.add"
            },
            json!({"channel": target.conversation, "timestamp": message_id, "name": emoji}),
        ),
        ChannelEffect::Edit { message_id, body } => {
            body.validate()?;
            (
                "chat.update",
                json!({"channel": target.conversation, "ts": message_id, "text": body.text}),
            )
        }
        ChannelEffect::Delete { message_id } | ChannelEffect::Redact { message_id, .. } => (
            "chat.delete",
            json!({"channel": target.conversation, "ts": message_id}),
        ),
        ChannelEffect::Pin { message_id } => (
            "pins.add",
            json!({"channel": target.conversation, "timestamp": message_id}),
        ),
        ChannelEffect::Unpin { message_id } => (
            "pins.remove",
            json!({"channel": target.conversation, "timestamp": message_id}),
        ),
        ChannelEffect::MarkRead { .. } => {
            return Err(ChannelError::Unsupported("Slack mark-read effect".to_owned()).into());
        }
    };
    super::post(client, config, path, &body.to_string(), false).await?;
    Ok(())
}
