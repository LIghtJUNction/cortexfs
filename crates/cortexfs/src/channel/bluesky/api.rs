use cortexfs_channels::OutboundRequest;
use reqwest::blocking::Client;
use serde_json::json;

use super::{BlueskyConfig, BlueskyError};
mod session;

pub(super) use session::{Session, login};

pub(super) fn notifications(
    client: &Client,
    config: &BlueskyConfig,
    session: &mut Session,
) -> Result<String, BlueskyError> {
    let token = session::access(client, config, session)?;
    client
        .get(format!(
            "{}/app.bsky.notification.listNotifications",
            config.api_base
        ))
        .bearer_auth(token)
        .query(&[("limit", "25")])
        .send()
        .map_err(BlueskyError::Http)?
        .error_for_status()
        .map_err(BlueskyError::Http)?
        .text()
        .map_err(BlueskyError::Http)
}

pub(super) fn mark_seen(
    client: &Client,
    config: &BlueskyConfig,
    session: &mut Session,
    seen_at: &str,
) -> Result<(), BlueskyError> {
    let token = session::access(client, config, session)?;
    client
        .post(format!(
            "{}/app.bsky.notification.updateSeen",
            config.api_base
        ))
        .bearer_auth(token)
        .json(&json!({"seenAt":seen_at}))
        .send()
        .map_err(BlueskyError::Http)?
        .error_for_status()
        .map_err(BlueskyError::Http)?;
    Ok(())
}

pub(super) fn send(
    client: &Client,
    config: &BlueskyConfig,
    session: &mut Session,
    request: OutboundRequest,
) -> Result<(), BlueskyError> {
    let token = session::access(client, config, session)?;
    client
        .post(format!("{}/{}", config.api_base, request.path))
        .bearer_auth(token)
        .header(reqwest::header::CONTENT_TYPE, request.content_type)
        .body(request.body)
        .send()
        .map_err(BlueskyError::Http)?
        .error_for_status()
        .map_err(BlueskyError::Http)?;
    Ok(())
}
