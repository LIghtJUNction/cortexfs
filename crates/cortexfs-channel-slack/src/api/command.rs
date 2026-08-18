#![expect(
    clippy::pattern_type_mismatch,
    reason = "matching borrowed provider-neutral commands keeps payloads borrowed"
)]

use std::fmt::Write as _;

use cortexfs_channels::{ChannelCommand, ChannelCommandResult, ChannelError, MessageTarget};
use serde_json::{Value, json};

use crate::{config::Config, error::Result, socket::PendingKind};

pub(crate) enum Outcome {
    Immediate(ChannelCommandResult),
    Pending(PendingKind),
}

pub(crate) fn pending_kind(command: &ChannelCommand) -> Option<PendingKind> {
    match command {
        ChannelCommand::RequestInput { .. } | ChannelCommand::RequestChoice { .. } => {
            Some(PendingKind::Input)
        }
        ChannelCommand::RequestApproval { .. } => Some(PendingKind::Approval),
        ChannelCommand::Notify { .. } | ChannelCommand::Invoke { .. } => None,
    }
}

pub(crate) async fn send(
    client: &reqwest::Client,
    config: &Config,
    target: &MessageTarget,
    command_id: &str,
    command: &ChannelCommand,
) -> Result<Outcome> {
    let body = body(target, command_id, command)?;
    super::post(client, config, "chat.postMessage", &body.to_string(), false).await?;
    Ok(pending_kind(command).map_or(
        Outcome::Immediate(ChannelCommandResult::Accepted),
        Outcome::Pending,
    ))
}

fn body(target: &MessageTarget, command_id: &str, command: &ChannelCommand) -> Result<Value> {
    let channel = target.conversation.as_str();
    let mut body = match command {
        ChannelCommand::Notify { level, text } => {
            json!({"channel": channel, "text": bounded(&format!("[{level}] {text}"))})
        }
        ChannelCommand::RequestInput { prompt } => {
            json!({"channel": channel, "text": bounded(prompt)})
        }
        ChannelCommand::RequestChoice {
            question,
            choices,
            multiple,
        } => json!({
            "channel": channel,
            "text": bounded(&choice_text(question, choices, *multiple)),
        }),
        ChannelCommand::RequestApproval { tool, .. } => json!({
            "channel": channel,
            "text": format!("Approve tool {}?", bounded(tool)),
            "blocks": approval_blocks(command_id, tool),
        }),
        ChannelCommand::Invoke { .. } => {
            return Err(ChannelError::Unsupported("Slack invoke command".to_owned()).into());
        }
    };
    if let Some(thread) = target.thread.as_deref() {
        let Some(fields) = body.as_object_mut() else {
            return Err(
                ChannelError::Protocol("Slack command body is not an object".to_owned()).into(),
            );
        };
        fields.insert("thread_ts".to_owned(), Value::String(thread.to_owned()));
    }
    Ok(body)
}

fn choice_text(
    question: &str,
    choices: &[cortexfs_channels::ChannelChoice],
    multiple: bool,
) -> String {
    let mode = if multiple {
        "Choose one or more by id:"
    } else {
        "Choose one by id:"
    };
    let mut text = format!("{}\n{}", bounded(question), mode);
    for choice in choices {
        let _ignored = write!(
            text,
            "\n- {}: {}",
            bounded(&choice.id),
            bounded(&choice.label)
        );
    }
    text
}

fn approval_blocks(command_id: &str, tool: &str) -> Value {
    let value = |result| json!({"command_id": command_id, "result": result}).to_string();
    json!([
        {"type":"section","text":{"type":"mrkdwn","text":format!("Approve tool {}?", bounded(tool))}},
        {"type":"actions","elements":[
            {"type":"button","text":{"type":"plain_text","text":"Approve"},"style":"primary","action_id":"cortexfs_command","value":value("accepted")},
            {"type":"button","text":{"type":"plain_text","text":"Reject"},"style":"danger","action_id":"cortexfs_command","value":value("rejected")}
        ]}
    ])
}

fn bounded(value: &str) -> String {
    value.chars().take(4_000).collect()
}
