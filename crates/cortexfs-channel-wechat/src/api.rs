#![expect(
    clippy::redundant_pub_crate,
    clippy::field_scoped_visibility_modifiers,
    reason = "HTTP helpers are private driver plumbing"
)]

use serde_json::{Value, json};

use crate::{config::Config, error::Result};

mod request;
use request::{check, client_id, headers};

#[cfg(test)]
mod tests;

pub(crate) struct UpdateBatch {
    pub(crate) cursor: String,
    pub(crate) messages: Vec<Value>,
}

pub(crate) async fn get_updates(
    client: &reqwest::Client,
    config: &Config,
    cursor: &str,
) -> Result<UpdateBatch> {
    let body = json!({
        "get_updates_buf": cursor,
        "base_info": {"channel_version": config.channel_version},
    });
    let value = post(client, config, "/ilink/bot/getupdates", &body).await?;
    let messages = value
        .get("msgs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(UpdateBatch {
        cursor: value
            .get("get_updates_buf")
            .and_then(Value::as_str)
            .unwrap_or(cursor)
            .to_owned(),
        messages,
    })
}

pub(crate) async fn send_message(
    client: &reqwest::Client,
    config: &Config,
    user: &str,
    context_token: &str,
    text: &str,
) -> Result<()> {
    let body = json!({
        "msg": {
            "from_user_id": "",
            "to_user_id": user,
            "client_id": client_id(),
            "message_type": 2,
            "message_state": 2,
            "item_list": [{"type": 1, "text_item": {"text": text}}],
            "context_token": context_token,
        },
        "base_info": {"channel_version": config.channel_version},
    });
    post(client, config, "/ilink/bot/sendmessage", &body).await?;
    Ok(())
}

async fn post(
    client: &reqwest::Client,
    config: &Config,
    path: &str,
    body: &Value,
) -> Result<Value> {
    let value = client
        .post(format!("{}{}", config.api_base, path))
        .headers(headers(config)?)
        .json(body)
        .send()
        .await?
        .json::<Value>()
        .await?;
    check(&value)?;
    Ok(value)
}

pub(crate) fn client(config: &Config) -> Result<reqwest::Client> {
    request::client(config)
}
