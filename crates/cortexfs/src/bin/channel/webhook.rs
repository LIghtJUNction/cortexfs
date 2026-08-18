use cortexfs::channel::{
    bridge::AgentChannelBridge,
    http::{HttpRequest, HttpResponse},
};
use cortexfs_channels::{
    ChannelCodec, ChannelId, ChannelIncoming,
    platform::{
        discord::DiscordCodec, feishu::FeishuCodec, line::LineCodec, linq::LinqCodec,
        nextcloud::NextcloudCodec, slack::SlackCodec, teams::TeamsCodec, wecom::WeComCodec,
        whatsapp::WhatsAppCodec,
    },
};
use serde_json::json;
use std::{fmt, net::SocketAddr};

mod challenge;
mod outbound;
mod progress;
mod server;
mod signature;

#[cfg(test)]
mod tests;
use super::config::Platform;
use challenge::handle as challenge;

/// Foreground webhook host configuration.
#[derive(Clone)]
pub struct WebhookConfig {
    pub bind: SocketAddr,
    pub path: String,
    pub platform: Platform,
    pub outbound_url: String,
    pub token: Option<String>,
    pub verify_token: Option<String>,
    pub channel: Option<ChannelId>,
}

impl fmt::Debug for WebhookConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebhookConfig")
            .field("bind", &self.bind)
            .field("path", &self.path)
            .field("platform", &self.platform)
            .field("outbound_url", &self.outbound_url)
            .field("token", &self.token.as_ref().map(|_| "[redacted]"))
            .field(
                "verify_token",
                &self.verify_token.as_ref().map(|_| "[redacted]"),
            )
            .field("channel", &self.channel)
            .finish()
    }
}

pub fn run(config: &WebhookConfig, bridge: &AgentChannelBridge) -> Result<(), WebhookError> {
    server::run(config, bridge)
}

/// Errors returned by the foreground webhook host.
#[derive(Debug, thiserror::Error)]
pub enum WebhookError {
    #[error("webhook I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("webhook HTTP request failed")]
    Http(#[source] reqwest::Error),
    #[error("webhook worker queue closed")]
    QueueClosed,
}

fn codec(platform: Platform) -> Box<dyn ChannelCodec> {
    match platform {
        Platform::Discord => Box::new(DiscordCodec),
        Platform::Slack => Box::new(SlackCodec),
        Platform::Feishu => Box::new(FeishuCodec),
        Platform::Line => Box::new(LineCodec),
        Platform::Linq => Box::new(LinqCodec),
        Platform::Nextcloud => Box::new(NextcloudCodec),
        Platform::Teams => Box::new(TeamsCodec),
        Platform::WhatsApp => Box::new(WhatsAppCodec),
        Platform::WeCom => Box::new(WeComCodec),
    }
}

#[expect(
    clippy::pattern_type_mismatch,
    reason = "match ergonomics keep the inbound event borrowed"
)]
fn handle(
    config: &WebhookConfig,
    client: &reqwest::blocking::Client,
    codec: &dyn ChannelCodec,
    bridge: &AgentChannelBridge,
    request: &HttpRequest,
) -> HttpResponse {
    if let Some(response) = challenge(config, request) {
        return response;
    }
    if request.method != "POST" || request.path != config.path {
        return HttpResponse::error(404, "not found");
    }
    if !signature::verify(config, request) {
        return HttpResponse::error(401, "invalid webhook signature");
    }
    if let Some(challenge) = codec.challenge(&request.body) {
        return HttpResponse::json(json!({ "challenge": challenge }).to_string());
    }
    let decoded = config.channel.as_ref().map_or_else(
        || codec.decode_many_incoming(&request.body),
        |channel| codec.decode_many_incoming_for(channel.clone(), &request.body),
    );
    let inbound = match decoded {
        Ok(inbound) if inbound.is_empty() => {
            return HttpResponse::json(json!({ "ok": true }).to_string());
        }
        Ok(inbound) => inbound,
        Err(_) => return HttpResponse::error(400, "invalid platform payload"),
    };
    for inbound in inbound {
        let target = match &inbound {
            ChannelIncoming::Message(message) => message.target.clone(),
            ChannelIncoming::Event(event) => event.context().target.clone(),
        };
        let mut progress = progress::Progress::new(client, config, codec, target);
        let Ok(outbound) = bridge.handle_incoming_with_progress(inbound, &mut progress) else {
            return HttpResponse::error(500, "agent delivery failed");
        };
        let Ok(outbound_request) = codec.encode(&outbound) else {
            return HttpResponse::error(500, "platform encoding failed");
        };
        if outbound::send(client, config, outbound_request).is_err() {
            return HttpResponse::error(502, "platform delivery failed");
        }
    }
    HttpResponse::json(json!({ "ok": true }).to_string())
}
