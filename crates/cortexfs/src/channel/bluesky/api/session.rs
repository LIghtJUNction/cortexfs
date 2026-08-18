use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::super::{BlueskyConfig, BlueskyError};

pub(in crate::channel::bluesky) struct Session {
    pub(in crate::channel::bluesky) access: String,
    pub(in crate::channel::bluesky) refresh: String,
    pub(in crate::channel::bluesky) did: String,
    expires: Instant,
}

pub(in crate::channel::bluesky) fn login(
    client: &Client,
    config: &BlueskyConfig,
) -> Result<Session, BlueskyError> {
    let response = client
        .post(format!(
            "{}/com.atproto.server.createSession",
            config.api_base
        ))
        .json(&json!({"identifier":config.handle,"password":config.app_password}))
        .send()
        .map_err(BlueskyError::Http)?
        .error_for_status()
        .map_err(BlueskyError::Http)?;
    let value = response.json::<Value>().map_err(BlueskyError::Http)?;
    session(&value)
}

fn session(value: &Value) -> Result<Session, BlueskyError> {
    Ok(Session {
        access: field(value, "accessJwt")?,
        refresh: field(value, "refreshJwt")?,
        did: field(value, "did")?,
        expires: Instant::now() + Duration::from_mins(90),
    })
}

pub(super) fn access(
    client: &Client,
    config: &BlueskyConfig,
    session: &mut Session,
) -> Result<String, BlueskyError> {
    if Instant::now() < session.expires && !session.access.is_empty() {
        return Ok(session.access.clone());
    }
    let response = client
        .post(format!(
            "{}/com.atproto.server.refreshSession",
            config.api_base
        ))
        .bearer_auth(&session.refresh)
        .send()
        .map_err(BlueskyError::Http)?;
    if !response.status().is_success() {
        return Err(BlueskyError::Unauthorized);
    }
    let value = response.json::<Value>().map_err(BlueskyError::Http)?;
    session.access = field(&value, "accessJwt")?;
    session.refresh = field(&value, "refreshJwt")?;
    session.expires = Instant::now() + Duration::from_mins(90);
    Ok(session.access.clone())
}

fn field(value: &Value, name: &str) -> Result<String, BlueskyError> {
    value
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| BlueskyError::Protocol(format!("Bluesky field `{name}` is missing")))
}
