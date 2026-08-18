use super::{ChannelCodec, OutboundRequest, irc::core};
use crate::{ChannelError, ChannelId, InboundMessage, OutboundMessage};

/// Twitch IRC codec. Connection security and OAuth registration stay host-owned.
#[derive(Clone, Copy, Debug, Default)]
pub struct TwitchCodec;

impl ChannelCodec for TwitchCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("twitch")
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        core::decode(self.channel(), payload)
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        core::encode("twitch", message)
    }
}

#[must_use]
pub fn normalize_oauth_token(raw: &str) -> String {
    let token = raw.trim();
    if token.starts_with("oauth:") {
        token.to_owned()
    } else {
        format!("oauth:{token}")
    }
}

#[must_use]
pub fn normalize_channel(raw: &str) -> Option<String> {
    let name = raw.trim().trim_start_matches('#').to_ascii_lowercase();
    (!name.is_empty()).then(|| format!("#{name}"))
}
