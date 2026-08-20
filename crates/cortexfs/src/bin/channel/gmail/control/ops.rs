use std::collections::BTreeMap;

use cortexfs_channels::{
    ChannelCodec, ConversationId, MessageBody, MessageTarget, OutboundMessage,
    platform::gmail::GmailCodec,
};
use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::super::{GmailConfig, api};
use cortexfs::channel::control::ChannelControlError;

pub(super) fn run(
    client: &Client,
    config: &GmailConfig,
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
) -> Result<Value, ChannelControlError> {
    let gmail = api::GmailApi::new(client, &config.api_base, &config.access_token);
    match name {
        "gmail.search" => gmail
            .search(value(payload, "query")?)
            .map_err(|error| operation(&error)),
        "gmail.read" | "gmail.fetch_message" => gmail
            .message(value(payload, "message_id")?)
            .map_err(|error| operation(&error)),
        "gmail.fetch_history" => history(&gmail, value(payload, "history_id")?),
        "gmail.mark_read" => gmail
            .modify(
                value(payload, "message_id")?,
                &json!({"removeLabelIds":["UNREAD"]}),
            )
            .map_err(|error| operation(&error)),
        "gmail.register_watch" => gmail.watch(payload).map_err(|error| operation(&error)),
        "gmail.reply" | "gmail.forward" => send_text(client, config, target, name, payload),
        "gmail.send_attachment" => {
            let raw = value(payload, "raw_base64")?;
            gmail
                .send(cortexfs_channels::OutboundRequest {
                    method: "POST".to_owned(),
                    path: "users/me/messages/send".to_owned(),
                    content_type: "application/json".to_owned(),
                    body: json!({"raw":raw}).to_string(),
                    headers: BTreeMap::new(),
                })
                .map_err(|error| operation(&error))
                .map(|()| json!({"accepted":true}))
        }
        _ => Err(fail("unsupported operation".to_owned())),
    }
}

fn send_text(
    client: &Client,
    config: &GmailConfig,
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
) -> Result<Value, ChannelControlError> {
    let mut target = target
        .cloned()
        .ok_or_else(|| fail("target is missing".to_owned()))?;
    if let Some(thread) = payload.get("thread_id").and_then(Value::as_str) {
        target.conversation =
            ConversationId::new(thread).map_err(|error| fail(error.to_string()))?;
    }
    let mut metadata = BTreeMap::new();
    metadata.insert("email.from".to_owned(), value(payload, "to")?.to_owned());
    metadata.insert(
        "email.subject".to_owned(),
        payload
            .get("subject")
            .and_then(Value::as_str)
            .unwrap_or(if name == "gmail.forward" {
                "Fwd: CortexFS"
            } else {
                "Re: CortexFS"
            })
            .to_owned(),
    );
    let message = OutboundMessage {
        target,
        body: MessageBody::text(value(payload, "text")?)
            .map_err(|error| fail(error.to_string()))?,
        metadata,
    };
    let request = GmailCodec
        .encode(&message)
        .map_err(|error| fail(error.to_string()))?;
    api::GmailApi::new(client, &config.api_base, &config.access_token)
        .send(request)
        .map_err(|error| operation(&error))
        .map(|()| json!({"accepted":true}))
}

fn history(gmail: &api::GmailApi<'_>, id: &str) -> Result<Value, ChannelControlError> {
    let result = gmail.history(id).map_err(|error| operation(&error))?;
    Ok(json!({
        "message_ids": result.message_ids(),
        "next_history_id": result.next_history_id(),
    }))
}

fn value<'a>(payload: &'a Value, name: &'static str) -> Result<&'a str, ChannelControlError> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && !value.contains('\0'))
        .ok_or_else(|| fail(format!("{name} is missing")))
}

fn fail(error: String) -> ChannelControlError {
    ChannelControlError::Operation(error)
}

fn operation(error: &super::super::GmailError) -> ChannelControlError {
    fail(error.to_string())
}
