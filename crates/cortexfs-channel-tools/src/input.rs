use cortexfs_channels::{
    Attachment, ChannelChoice, ChannelCommand, ChannelId, ConversationId, MessageBody,
    MessageTarget,
};
use cortexfs_tool_sdk::{ToolError, ToolResult};
use serde_json::Value;
pub(super) fn target(input: &Value, channel: &str) -> ToolResult<MessageTarget> {
    let conversation = std::env::var("CTX_CHANNEL_CONVERSATION")
        .ok()
        .or_else(|| optional_string(input, "conversation"))
        .or_else(|| std::env::var("CTX_CHANNEL_SESSION").ok())
        .ok_or_else(|| ToolError::invalid("missing string field: conversation"))?;
    Ok(MessageTarget {
        channel: ChannelId::new(channel).map_err(|error| ToolError::invalid(error.to_string()))?,
        conversation: ConversationId::new(conversation)
            .map_err(|error| ToolError::invalid(error.to_string()))?,
        thread: optional_string(input, "thread"),
        reply_to: optional_string(input, "reply_to"),
    })
}

pub(super) fn body(input: &Value) -> ToolResult<MessageBody> {
    let attachments: Vec<Attachment> = input
        .get("attachments")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| ToolError::invalid(format!("invalid attachments: {error}")))?
        .unwrap_or_default();
    MessageBody::with_attachments(
        input
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        attachments,
    )
    .map_err(|error| ToolError::invalid(error.to_string()))
}

pub(super) fn choice(input: &Value) -> ToolResult<ChannelCommand> {
    Ok(ChannelCommand::RequestChoice {
        question: string(input, "question")?,
        choices: choices(input)?,
        multiple: bool_field(input, "multiple", false),
    })
}

pub(super) fn multi_choice(input: &Value) -> ToolResult<ChannelCommand> {
    Ok(ChannelCommand::RequestChoice {
        question: string(input, "question")?,
        choices: choices(input)?,
        multiple: true,
    })
}

fn choices(input: &Value) -> ToolResult<Vec<ChannelChoice>> {
    serde_json::from_value(
        input
            .get("choices")
            .cloned()
            .ok_or_else(|| ToolError::invalid("missing choices"))?,
    )
    .map_err(|error| ToolError::invalid(format!("invalid choices: {error}")))
}

pub(super) fn approval(input: &Value) -> ToolResult<ChannelCommand> {
    Ok(ChannelCommand::RequestApproval {
        tool: string(input, "tool")?,
        arguments: input.get("arguments").cloned().unwrap_or(Value::Null),
    })
}

pub(super) fn notify(input: &Value) -> ToolResult<ChannelCommand> {
    Ok(ChannelCommand::Notify {
        level: string(input, "level")?,
        text: string(input, "text")?,
    })
}

pub(super) fn string(input: &Value, name: &str) -> ToolResult<String> {
    input
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ToolError::invalid(format!("missing string field: {name}")))
}

pub(super) fn optional_string(input: &Value, name: &str) -> Option<String> {
    input
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub(super) fn bool_field(input: &Value, name: &str, default: bool) -> bool {
    input.get(name).and_then(Value::as_bool).unwrap_or(default)
}
