use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::super::api;
use super::super::{NotionConfig, NotionError};
use cortexfs_channels::{OutboundRequest, platform::notion::NotionCodec};

pub(super) fn mark_running(
    client: &Client,
    config: &NotionConfig,
    codec: &NotionCodec,
    page_id: &str,
) -> Result<(), NotionError> {
    update_status(
        client,
        config,
        page_id,
        codec.status_type(),
        "running",
        None,
    )
}

pub(super) fn mark_failed(
    client: &Client,
    config: &NotionConfig,
    codec: &NotionCodec,
    page_id: &str,
    detail: &str,
) -> Result<(), NotionError> {
    update_status(
        client,
        config,
        page_id,
        codec.status_type(),
        "failed",
        Some(detail),
    )
}

pub(super) fn update_status(
    client: &Client,
    config: &NotionConfig,
    page_id: &str,
    status_type: &str,
    status: &str,
    result: Option<&str>,
) -> Result<(), NotionError> {
    let status_key = if status_type == "status" {
        "status"
    } else {
        "select"
    };
    let mut properties = serde_json::Map::new();
    properties.insert(
        config.status_property.clone(),
        json!({status_key: {"name": status}}),
    );
    if let Some(result) = result {
        properties.insert(
            config.result_property.clone(),
            json!({"rich_text": [{"text": {"content": result}}]}),
        );
    }
    api::request(
        client,
        config,
        reqwest::Method::PATCH,
        &format!("pages/{page_id}"),
        Some(json!({"properties": properties})),
    )?;
    Ok(())
}

pub(super) fn send_outbound(
    client: &Client,
    config: &NotionConfig,
    request: &OutboundRequest,
) -> Result<(), NotionError> {
    if request.method != "PATCH" {
        return Err(NotionError::Protocol(
            "Notion outbound method is invalid".to_owned(),
        ));
    }
    let body: Value = serde_json::from_str(&request.body)
        .map_err(|error| NotionError::Protocol(error.to_string()))?;
    api::request(
        client,
        config,
        reqwest::Method::PATCH,
        &request.path,
        Some(body),
    )?;
    Ok(())
}
