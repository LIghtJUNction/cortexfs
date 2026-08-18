use reqwest::blocking::Client;
use serde::Deserialize;

use super::{DingTalkConfig, DingTalkError};
use cortexfs_channels::OutboundRequest;

pub(super) const TOPIC: &str = "/v1.0/im/bot/messages/get";

#[derive(Deserialize)]
pub(super) struct GatewayResponse {
    pub(super) endpoint: String,
    pub(super) ticket: String,
}

pub(super) fn register(
    client: &Client,
    config: &DingTalkConfig,
) -> Result<GatewayResponse, DingTalkError> {
    client.post(&config.gateway_url)
        .json(&serde_json::json!({"clientId":config.client_id,"clientSecret":config.client_secret,"subscriptions":[{"type":"CALLBACK","topic":TOPIC}]}))
        .send().map_err(DingTalkError::Http)?.error_for_status().map_err(DingTalkError::Http)?
        .json().map_err(DingTalkError::Http)
}

pub(super) fn reply(
    client: &Client,
    webhook: &str,
    request: OutboundRequest,
) -> Result<(), DingTalkError> {
    client
        .post(webhook)
        .header(reqwest::header::CONTENT_TYPE, request.content_type)
        .body(request.body)
        .send()
        .map_err(DingTalkError::Http)?
        .error_for_status()
        .map_err(DingTalkError::Http)?;
    Ok(())
}
