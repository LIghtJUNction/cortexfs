use cortexfs_channels::MessageTarget;
use reqwest::Client;
use serde_json::Value;

use crate::{config::Config, error::Result};

pub(crate) async fn run(
    client: &Client,
    config: &Config,
    target: &MessageTarget,
    name: &str,
    payload: &Value,
) -> Result<Value> {
    let mut body = payload.as_object().cloned().unwrap_or_default();
    body.entry("channel".to_owned())
        .or_insert_with(|| Value::String(target.conversation.to_string()));
    let path = match name {
        "slack.send_blocks" => "chat.postMessage",
        "slack.upload_file" => "files.uploadV2",
        "slack.post_ephemeral" => "chat.postEphemeral",
        "slack.open_modal" => "views.open",
        "slack.list_channels" => "conversations.list",
        "slack.thread_reply" => {
            let thread = body
                .get("thread_ts")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| crate::error::Error::Api("thread_ts is missing".to_owned()))?;
            body.insert("thread_ts".to_owned(), Value::String(thread.to_owned()));
            "chat.postMessage"
        }
        "slack.draft_update" => "chat.update",
        _ => return Err(crate::error::Error::Api("unsupported operation".to_owned())),
    };
    super::post(
        client,
        config,
        path,
        &Value::Object(body).to_string(),
        false,
    )
    .await
}
