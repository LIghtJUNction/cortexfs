use std::{collections::BTreeSet, net::TcpListener};

use cortexfs::channel::{
    bridge::{AgentChannelBridge, ChannelBridgeError},
    http::{self, HttpRequest, HttpResponse},
};
use cortexfs_channels::{ChannelCodec, ChannelError, platform::gmail::GmailCodec};
use reqwest::blocking::Client;
use serde_json::json;

mod api;
mod config;
mod control;

pub use config::GmailConfig;

#[derive(Debug, thiserror::Error)]
pub enum GmailError {
    #[error("Gmail host I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Gmail API request failed")]
    Http(#[source] reqwest::Error),
    #[error("Gmail API returned an invalid response: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Channel(#[from] ChannelError),
    #[error(transparent)]
    Bridge(#[from] ChannelBridgeError),
    #[error("Gmail API rejected request: {0}")]
    Api(String),
}

pub fn run(config: &GmailConfig, bridge: &AgentChannelBridge) -> Result<(), GmailError> {
    let listener = TcpListener::bind(config.bind)?;
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(GmailError::Http)?;
    let control = control::start(config, bridge, &client)?;
    let mut history = None;
    let mut seen = BTreeSet::new();
    loop {
        control
            .check()
            .map_err(|error| GmailError::Api(error.to_string()))?;
        match http::serve_once(&listener, |request| {
            handle(config, &client, bridge, &mut history, &mut seen, &request)
        }) {
            Ok(()) | Err(http::HttpError::Invalid(_)) => {}
            Err(http::HttpError::Io(error)) => return Err(GmailError::Io(error)),
        }
    }
}

fn handle(
    config: &GmailConfig,
    client: &Client,
    bridge: &AgentChannelBridge,
    history: &mut Option<String>,
    seen: &mut BTreeSet<String>,
    request: &HttpRequest,
) -> HttpResponse {
    if request.method != "POST" || request.path != config.path {
        return HttpResponse::error(404, "not found");
    }
    if config.token.as_deref().is_some_and(|token| {
        request
            .headers
            .get("authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
            != Some(token)
    }) {
        return HttpResponse::error(401, "unauthorized");
    }
    let codec = GmailCodec;
    let Ok(Some(push)) = GmailCodec::push_cursor(&request.body) else {
        return HttpResponse::error(400, "invalid Gmail push");
    };
    if history.as_deref() == Some(push.history_id.as_str()) {
        return HttpResponse::json(json!({"ok": true, "duplicate": true}).to_string());
    }
    let api = api::GmailApi::new(client, &config.api_base, &config.access_token);
    let result = process_push(&api, bridge, codec, &push, history, seen);
    match result {
        Ok(()) => HttpResponse::json(json!({"ok": true}).to_string()),
        Err(_) => HttpResponse::error(502, "Gmail delivery failed"),
    }
}

fn process_push(
    api: &api::GmailApi<'_>,
    bridge: &AgentChannelBridge,
    codec: GmailCodec,
    push: &cortexfs_channels::platform::gmail::GmailPush,
    history: &mut Option<String>,
    seen: &mut BTreeSet<String>,
) -> Result<(), GmailError> {
    let batch = api.history(&push.history_id)?;
    for id in batch.message_ids().iter().cloned() {
        if seen.contains(&id) {
            continue;
        }
        let resource = api.message(&id)?;
        if let Some(inbound) = codec
            .decode(&resource.to_string())?
            .filter(|message| !message.sender.id.eq_ignore_ascii_case(&push.email_address))
            && let Some(outbound) = ChannelBridgeError::consume_denied(bridge.handle(inbound))?
        {
            api.send(codec.encode(&outbound)?)?;
        }
        seen.insert(id);
    }
    if seen.len() > 4096 {
        seen.clear();
    }
    *history = Some(
        batch
            .next_history_id()
            .map_or_else(|| push.history_id.clone(), str::to_owned),
    );
    Ok(())
}
