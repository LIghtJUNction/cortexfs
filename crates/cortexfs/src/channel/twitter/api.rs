use cortexfs_channels::OutboundRequest;
use reqwest::{StatusCode, blocking::Client};
use serde_json::Value;

use super::{TwitterConfig, TwitterError};

pub(super) fn me(client: &Client, config: &TwitterConfig) -> Result<String, TwitterError> {
    let value = checked(
        client
            .get(url(&config.api_base, "users/me"))
            .bearer_auth(&config.bearer_token)
            .send()
            .map_err(TwitterError::Http)?,
    )?
    .json::<Value>()
    .map_err(TwitterError::Http)?;
    field(value.get("data"), "id")
}

pub(super) fn mentions(
    client: &Client,
    config: &TwitterConfig,
    bot_id: &str,
    since_id: Option<&str>,
) -> Result<String, TwitterError> {
    let mut query = vec![
        ("tweet.fields", "author_id,conversation_id,created_at"),
        ("expansions", "author_id"),
        ("user.fields", "username,name"),
        ("max_results", "100"),
    ];
    if let Some(since_id) = since_id {
        query.push(("since_id", since_id));
    }
    checked(
        client
            .get(url(&config.api_base, &format!("users/{bot_id}/mentions")))
            .bearer_auth(&config.bearer_token)
            .query(&query)
            .send()
            .map_err(TwitterError::Http)?,
    )?
    .text()
    .map_err(TwitterError::Http)
}

pub(super) fn send(
    client: &Client,
    config: &TwitterConfig,
    request: OutboundRequest,
) -> Result<Option<String>, TwitterError> {
    let mut builder = client
        .post(url(&config.api_base, &request.path))
        .bearer_auth(&config.bearer_token)
        .header(reqwest::header::CONTENT_TYPE, request.content_type)
        .body(request.body);
    for (name, value) in request.headers {
        builder = builder.header(name, value);
    }
    let value = checked(builder.send().map_err(TwitterError::Http)?)?
        .json::<Value>()
        .map_err(TwitterError::Http)?;
    Ok(value
        .get("data")
        .and_then(|data| data.get("id"))
        .and_then(Value::as_str)
        .map(str::to_owned))
}

fn checked(
    response: reqwest::blocking::Response,
) -> Result<reqwest::blocking::Response, TwitterError> {
    if response.status() == StatusCode::TOO_MANY_REQUESTS {
        return Err(TwitterError::RateLimited);
    }
    response.error_for_status().map_err(TwitterError::Http)
}

fn field(value: Option<&Value>, name: &str) -> Result<String, TwitterError> {
    value
        .and_then(|value| value.get(name))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| TwitterError::Protocol(format!("Twitter field `{name}` is missing")))
}

fn url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}
