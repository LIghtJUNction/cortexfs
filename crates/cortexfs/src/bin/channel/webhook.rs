use std::time::Duration;
use std::{
    fmt,
    net::{SocketAddr, TcpListener},
};

use cortexfs::channel::{
    bridge::AgentChannelBridge,
    http::{self, HttpRequest, HttpResponse},
};
use cortexfs_channels::{
    ChannelCodec,
    platform::{discord::DiscordCodec, feishu::FeishuCodec, slack::SlackCodec},
};
use serde_json::json;

mod outbound;

use super::config::Platform;

/// Foreground webhook host configuration.
pub struct WebhookConfig {
    pub bind: SocketAddr,
    pub path: String,
    pub platform: Platform,
    pub outbound_url: String,
    pub token: Option<String>,
}

impl fmt::Debug for WebhookConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebhookConfig")
            .field("bind", &self.bind)
            .field("path", &self.path)
            .field("platform", &self.platform)
            .field("outbound_url", &self.outbound_url)
            .field("token", &self.token.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

pub fn run(config: &WebhookConfig, bridge: &AgentChannelBridge) -> Result<(), WebhookError> {
    let listener = TcpListener::bind(config.bind).map_err(WebhookError::Io)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_mins(1))
        .build()
        .map_err(WebhookError::Http)?;
    let codec = codec(config.platform);
    loop {
        match http::serve_once(&listener, |request| {
            handle(config, &client, codec.as_ref(), bridge, &request)
        }) {
            Ok(()) | Err(http::HttpError::Invalid(_)) => {}
            Err(http::HttpError::Io(error)) => return Err(WebhookError::Io(error)),
        }
    }
}

/// Errors returned by the foreground webhook host.
#[derive(Debug, thiserror::Error)]
pub enum WebhookError {
    #[error("webhook I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("webhook HTTP request failed")]
    Http(#[source] reqwest::Error),
}

fn codec(platform: Platform) -> Box<dyn ChannelCodec> {
    match platform {
        Platform::Discord => Box::new(DiscordCodec),
        Platform::Slack => Box::new(SlackCodec),
        Platform::Feishu => Box::new(FeishuCodec),
    }
}

fn handle(
    config: &WebhookConfig,
    client: &reqwest::blocking::Client,
    codec: &dyn ChannelCodec,
    bridge: &AgentChannelBridge,
    request: &HttpRequest,
) -> HttpResponse {
    if request.method != "POST" || request.path != config.path {
        return HttpResponse::error(404, "not found");
    }
    if let Some(challenge) = codec.challenge(&request.body) {
        return HttpResponse::json(json!({ "challenge": challenge }).to_string());
    }
    let inbound = match codec.decode(&request.body) {
        Ok(Some(inbound)) => inbound,
        Ok(None) => return HttpResponse::json(json!({ "ok": true }).to_string()),
        Err(_) => return HttpResponse::error(400, "invalid platform payload"),
    };
    let Ok(outbound) = bridge.handle(inbound) else {
        return HttpResponse::error(500, "agent delivery failed");
    };
    let Ok(outbound_request) = codec.encode(&outbound) else {
        return HttpResponse::error(500, "platform encoding failed");
    };
    if outbound::send(client, config, outbound_request).is_err() {
        return HttpResponse::error(502, "platform delivery failed");
    }
    HttpResponse::json(json!({ "ok": true }).to_string())
}
