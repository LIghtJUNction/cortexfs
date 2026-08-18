use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::super::api;
use super::super::{NotionConfig, NotionError};

pub(super) fn status_type(client: &Client, config: &NotionConfig) -> Result<String, NotionError> {
    let value = api::request(
        client,
        config,
        reqwest::Method::GET,
        &format!("databases/{}", config.database_id),
        None,
    )?;
    let value = value
        .get("properties")
        .and_then(|properties| properties.get(&config.status_property))
        .and_then(|property| property.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("select");
    Ok(if value == "status" {
        "status"
    } else {
        "select"
    }
    .to_owned())
}

pub(super) fn pending(
    client: &Client,
    config: &NotionConfig,
    status_type: &str,
) -> Result<Vec<Value>, NotionError> {
    let filter = if status_type == "status" {
        json!({"property": config.status_property, "status": {"equals": "pending"}})
    } else {
        json!({"property": config.status_property, "select": {"equals": "pending"}})
    };
    let value = api::request(
        client,
        config,
        reqwest::Method::POST,
        &format!("databases/{}/query", config.database_id),
        Some(json!({"filter": filter})),
    )?;
    Ok(value
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

pub(super) fn recover_stale(
    client: &Client,
    config: &NotionConfig,
    status_type: &str,
) -> Result<(), NotionError> {
    let filter = if status_type == "status" {
        json!({"property": config.status_property, "status": {"equals": "running"}})
    } else {
        json!({"property": config.status_property, "select": {"equals": "running"}})
    };
    let value = api::request(
        client,
        config,
        reqwest::Method::POST,
        &format!("databases/{}/query", config.database_id),
        Some(json!({"filter": filter})),
    )?;
    for page in value
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(id) = page.get("id").and_then(Value::as_str) {
            super::write::update_status(client, config, id, status_type, "pending", None)?;
        }
    }
    Ok(())
}
