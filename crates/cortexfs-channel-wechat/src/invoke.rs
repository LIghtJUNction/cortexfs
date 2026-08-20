use cortexfs_channels::MessageTarget;
use reqwest::Client;
use serde_json::{Value, json};

use crate::{
    api,
    config::Config,
    error::{Error, Result},
};

#[expect(clippy::redundant_pub_crate, reason = "private driver helper")]
pub(crate) async fn run(
    client: &Client,
    config: &Config,
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
) -> Result<Value> {
    let user = payload
        .get("user_id")
        .and_then(Value::as_str)
        .or_else(|| target.map(|item| item.conversation.as_str()))
        .ok_or_else(|| error("user_id is missing"))?;
    let context = payload
        .get("context_token")
        .and_then(Value::as_str)
        .unwrap_or("");
    match name {
        "wechat.send_markdown" => {
            let text = payload
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| error("text is missing"))?;
            api::send_message(client, config, user, context, text).await?;
            Ok(json!({"accepted":true}))
        }
        _ => Err(error("unsupported operation")),
    }
}

fn error(message: &str) -> Error {
    Error::Protocol(message.to_owned())
}
