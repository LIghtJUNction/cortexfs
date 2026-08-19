use crate::input::{body, string};
use cortexfs_channels::{
    ChannelCommand, ChannelControlAction, ChannelEffect, ChannelId, MessageTarget, OutboundMessage,
};
use cortexfs_tool_sdk::{ToolError, ToolInvocation, ToolResult};
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::time::Duration;

pub(super) fn request(channel: &str, run: &str, action: ChannelControlAction) -> ToolResult<()> {
    let socket = env::var("CTX_CHANNEL_SOCKET").unwrap_or_else(|_| {
        cortexfs_paths::channel_driver_socket(channel)
            .display()
            .to_string()
    });
    let id = format!("tool-{run}");
    let channel = ChannelId::new(channel).map_err(|error| ToolError::invalid(error.to_string()))?;
    let mut client = cortexfs_channels::ChannelDriverClient::connect_control_retry(
        &std::path::PathBuf::from(socket),
        &channel,
        &id,
        Duration::from_secs(5),
    )
    .map_err(|error| ToolError::new("EIO", error.to_string()))?;
    client
        .request_control(&id, action)
        .map_err(|error| ToolError::new("EIO", error.to_string()))
}

pub(super) fn send(
    input: &Value,
    target: MessageTarget,
    reply: bool,
) -> ToolResult<ChannelControlAction> {
    let mut target = target;
    if reply {
        target.reply_to = Some(string(input, "message_id")?);
    }
    Ok(ChannelControlAction::Send {
        message: OutboundMessage {
            target,
            body: body(input)?,
            metadata: BTreeMap::new(),
        },
    })
}

pub(super) fn effect(target: MessageTarget, effect: ChannelEffect) -> ChannelControlAction {
    ChannelControlAction::Effect { target, effect }
}

pub(super) fn command(
    invocation: &ToolInvocation,
    target: MessageTarget,
    command: ChannelCommand,
) -> ChannelControlAction {
    ChannelControlAction::Command {
        session: env::var("CTX_CHANNEL_SESSION").unwrap_or_else(|_| "default".to_owned()),
        command_id: invocation.run_id().to_owned(),
        command,
        target: Some(target),
    }
}

pub(super) fn invoke(
    invocation: &ToolInvocation,
    target: MessageTarget,
    name: &str,
    payload: Value,
) -> ChannelControlAction {
    command(
        invocation,
        target,
        ChannelCommand::Invoke {
            name: name.to_owned(),
            payload,
        },
    )
}
