use std::time::{Duration, Instant};

use cortexfs_channels::OutboundRequest;
use reqwest::blocking::Client;
use serde_json::Value;

use super::{RedditConfig, RedditError};

pub(super) struct Session {
    access: String,
    expires: Instant,
}
pub(super) fn login(client: &Client, config: &RedditConfig) -> Result<Session, RedditError> {
    let value = client
        .post(&config.token_url)
        .basic_auth(&config.client_id, Some(&config.client_secret))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", config.refresh_token.as_str()),
        ])
        .send()
        .map_err(RedditError::Http)?
        .error_for_status()
        .map_err(RedditError::Http)?
        .json::<Value>()
        .map_err(RedditError::Http)?;
    let access = value
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| RedditError::Protocol("access_token is missing".to_owned()))?;
    let seconds = value
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(3_600)
        .saturating_sub(60);
    Ok(Session {
        access: access.to_owned(),
        expires: Instant::now() + Duration::from_secs(seconds),
    })
}

fn access(
    client: &Client,
    config: &RedditConfig,
    session: &mut Session,
) -> Result<String, RedditError> {
    if Instant::now() < session.expires && !session.access.is_empty() {
        return Ok(session.access.clone());
    }
    *session = login(client, config)?;
    Ok(session.access.clone())
}

pub(super) fn inbox(
    client: &Client,
    config: &RedditConfig,
    session: &mut Session,
) -> Result<String, RedditError> {
    let token = access(client, config, session)?;
    client
        .get(url(&config.api_base, "message/unread"))
        .bearer_auth(token)
        .header("User-Agent", "cortexfs-channel/1")
        .query(&[("limit", "25")])
        .send()
        .map_err(RedditError::Http)?
        .error_for_status()
        .map_err(RedditError::Http)?
        .text()
        .map_err(RedditError::Http)
}

pub(super) fn send(
    client: &Client,
    config: &RedditConfig,
    session: &mut Session,
    request: OutboundRequest,
) -> Result<(), RedditError> {
    let token = access(client, config, session)?;
    client
        .post(url(&config.api_base, &request.path))
        .bearer_auth(token)
        .header(reqwest::header::CONTENT_TYPE, request.content_type)
        .body(request.body)
        .send()
        .map_err(RedditError::Http)?
        .error_for_status()
        .map_err(RedditError::Http)?;
    Ok(())
}

pub(super) fn mark_read(
    client: &Client,
    config: &RedditConfig,
    session: &mut Session,
    ids: &[String],
) -> Result<(), RedditError> {
    if ids.is_empty() {
        return Ok(());
    }
    let token = access(client, config, session)?;
    client
        .post(url(&config.api_base, "api/read_message"))
        .bearer_auth(token)
        .form(&[("id", ids.join(","))])
        .send()
        .map_err(RedditError::Http)?
        .error_for_status()
        .map_err(RedditError::Http)?;
    Ok(())
}

fn url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}
