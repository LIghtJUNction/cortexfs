use std::{thread, time::Duration};

use reqwest::blocking::{Client, RequestBuilder};
use serde_json::Value;

use super::{NotionConfig, NotionError};

mod query;
mod write;

pub(super) fn status_type(client: &Client, config: &NotionConfig) -> Result<String, NotionError> {
    query::status_type(client, config)
}

pub(super) fn pending(
    client: &Client,
    config: &NotionConfig,
    status_type: &str,
) -> Result<Vec<Value>, NotionError> {
    query::pending(client, config, status_type)
}

pub(super) fn recover_stale(
    client: &Client,
    config: &NotionConfig,
    status_type: &str,
) -> Result<(), NotionError> {
    query::recover_stale(client, config, status_type)
}

pub(super) fn mark_running(
    client: &Client,
    config: &NotionConfig,
    codec: &cortexfs_channels::platform::notion::NotionCodec,
    page_id: &str,
) -> Result<(), NotionError> {
    write::mark_running(client, config, codec, page_id)
}

pub(super) fn mark_failed(
    client: &Client,
    config: &NotionConfig,
    codec: &cortexfs_channels::platform::notion::NotionCodec,
    page_id: &str,
    detail: &str,
) -> Result<(), NotionError> {
    write::mark_failed(client, config, codec, page_id, detail)
}

pub(super) fn send_outbound(
    client: &Client,
    config: &NotionConfig,
    request: &cortexfs_channels::OutboundRequest,
) -> Result<(), NotionError> {
    write::send_outbound(client, config, request)
}

pub(super) fn request(
    client: &Client,
    config: &NotionConfig,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value, NotionError> {
    let mut builder = client
        .request(method, url(&config.api_base, path))
        .bearer_auth(&config.api_token)
        .header("Notion-Version", "2022-06-28")
        .header(reqwest::header::CONTENT_TYPE, "application/json");
    if let Some(body) = body {
        builder = builder.json(&body);
    }
    send(builder)
}

fn send(builder: RequestBuilder) -> Result<Value, NotionError> {
    let mut builder = builder;
    for attempt in 0..3 {
        let retry = builder.try_clone();
        let response = builder.send().map_err(NotionError::Http)?;
        let status = response.status();
        if status.is_success() {
            return response.json().map_err(NotionError::Http);
        }
        if !(status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error())
            || attempt == 2
        {
            return Err(NotionError::Protocol(format!("Notion API status {status}")));
        }
        builder = retry.ok_or_else(|| {
            NotionError::Protocol("Notion request could not be retried".to_owned())
        })?;
        thread::sleep(Duration::from_millis(200_u64 << attempt));
    }
    Err(NotionError::Protocol(
        "Notion request exhausted retries".to_owned(),
    ))
}

pub(super) fn url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}
