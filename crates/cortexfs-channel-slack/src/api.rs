#![expect(
    clippy::redundant_pub_crate,
    reason = "HTTP helpers are private driver plumbing"
)]

use std::time::Duration;

use cortexfs_channels::{
    ChannelCodec, DeliveryReceipt, OutboundMessage, platform::slack::SlackCodec,
};
use reqwest::Client;
use serde_json::{Value, json};

use crate::{
    config::Config,
    error::{Error, Result},
};

mod command;
mod effect;

#[cfg(test)]
mod tests;

pub(crate) use command::{Outcome as CommandOutcome, pending_kind, send as send_command};
pub(crate) use effect::apply as effect;

pub(crate) async fn open_url(client: &Client, config: &Config) -> Result<String> {
    let body = json!({}).to_string();
    let value = post(client, config, "apps.connections.open", &body, true).await?;
    value
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| Error::Api("Slack did not return a Socket Mode URL".to_owned()))
}

pub(crate) async fn send_message(
    client: &Client,
    config: &Config,
    message: OutboundMessage,
) -> Result<DeliveryReceipt> {
    let target = message.target.clone();
    let request = SlackCodec.encode(&message)?;
    let value = post(client, config, &request.path, &request.body, false).await?;
    let id = value
        .get("ts")
        .and_then(Value::as_str)
        .map_or_else(|| format!("slack-{}", target.conversation), str::to_owned);
    Ok(DeliveryReceipt {
        channel: target.channel.clone(),
        message_id: id,
        target,
        timestamp_ms: None,
    })
}

pub(super) async fn post(
    client: &Client,
    config: &Config,
    path: &str,
    body: &str,
    app_token: bool,
) -> Result<Value> {
    let url = format!("{}/{}", config.api_base.trim_end_matches('/'), path);
    let token = if app_token {
        &config.app_token
    } else {
        &config.bot_token
    };
    for attempt in 0..3 {
        let response = client
            .post(&url)
            .bearer_auth(token)
            .header("content-type", "application/json")
            .body(body.to_owned())
            .send()
            .await
            .map_err(Error::Http)?;
        if response.status().as_u16() == 429 {
            let delay = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(1)
                .min(60);
            if attempt < 2 {
                tokio::time::sleep(Duration::from_secs(delay)).await;
                continue;
            }
        }
        let value = response.json::<Value>().await.map_err(Error::Http)?;
        if value.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err(Error::Api(
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("request rejected")
                    .to_owned(),
            ));
        }
        return Ok(value);
    }
    Err(Error::Api("Slack rate limit retry exhausted".to_owned()))
}
