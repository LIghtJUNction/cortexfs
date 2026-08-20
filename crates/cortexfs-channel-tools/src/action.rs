use cortexfs_channels::{ChannelCommand, ChannelEffect};
use cortexfs_tool_sdk::{ToolError, ToolInvocation, ToolResult};
use serde_json::{Value, json};
use std::env;

use crate::input::{approval, body, bool_field, choice, multi_choice, notify, string, target};
use crate::wire::{command, effect, invoke, request, send};

pub(crate) fn run(name: &str, invocation: &ToolInvocation) -> ToolResult<Value> {
    let input = invocation.json()?;
    let channel = env::var("CTX_CHANNEL_ID")
        .map_err(|error| ToolError::invalid(format!("missing channel context: {error}")))?;
    let target = target(&input, &channel)?;
    let action = match name {
        "channel.send" => send(&input, target, false)?,
        "channel.reply" => send(&input, target, true)?,
        "channel.typing" => effect(
            target,
            ChannelEffect::Typing {
                active: bool_field(&input, "active", true),
            },
        ),
        "channel.preview" => effect(
            target,
            ChannelEffect::Preview {
                text: string(&input, "text")?,
            },
        ),
        "channel.react" => effect(
            target,
            ChannelEffect::Reaction {
                message_id: string(&input, "message_id")?,
                emoji: string(&input, "emoji")?,
                remove: bool_field(&input, "remove", false),
            },
        ),
        "channel.edit" => effect(
            target,
            ChannelEffect::Edit {
                message_id: string(&input, "message_id")?,
                body: body(&input)?,
            },
        ),
        "channel.delete" => effect(
            target,
            ChannelEffect::Delete {
                message_id: string(&input, "message_id")?,
            },
        ),
        "channel.mark_read" => effect(
            target,
            ChannelEffect::MarkRead {
                message_id: string(&input, "message_id")?,
            },
        ),
        "channel.pin" => effect(
            target,
            ChannelEffect::Pin {
                message_id: string(&input, "message_id")?,
            },
        ),
        "channel.unpin" => effect(
            target,
            ChannelEffect::Unpin {
                message_id: string(&input, "message_id")?,
            },
        ),
        "channel.redact" => effect(
            target,
            ChannelEffect::Redact {
                message_id: string(&input, "message_id")?,
                reason: input
                    .get("reason")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            },
        ),
        "channel.choice" => command(invocation, target, choice(&input)?),
        "channel.multi_choice" => command(invocation, target, multi_choice(&input)?),
        "channel.input" | "channel.ask" => command(
            invocation,
            target,
            ChannelCommand::RequestInput {
                prompt: string(&input, "prompt")?,
            },
        ),
        "channel.approval" => command(invocation, target, approval(&input)?),
        "channel.notify" => command(invocation, target, notify(&input)?),
        _ => invoke(invocation, target, name, input),
    };
    request(&channel, invocation.run_id(), action)?;
    Ok(json!({"accepted": true, "channel": channel, "tool": name}))
}
