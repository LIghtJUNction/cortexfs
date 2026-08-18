use serde::{Deserialize, Serialize};

use crate::{ChannelId, ChannelIncomingEvent, InboundMessage};

/// One provider-neutral item emitted by a channel receive stream.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ChannelIncoming {
    Message(InboundMessage),
    Event(ChannelIncomingEvent),
}

impl ChannelIncoming {
    /// Rebinds a decoded item to the configured channel instance.
    ///
    /// Stateless codecs own the platform family id; a host that runs several
    /// accounts uses this method to retain its complete `ChannelId` alias.
    #[must_use]
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "rebinding only mutates the borrowed target in place"
    )]
    pub fn with_channel(mut self, channel: ChannelId) -> Self {
        match &mut self {
            Self::Message(message) => message.target.channel = channel,
            Self::Event(event) => event.context_mut().target.channel = channel,
        }
        self
    }

    /// Returns a deterministic wire id for either kind of inbound item.
    #[must_use]
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "matching borrowed incoming items keeps event ids zero-copy"
    )]
    pub fn event_id(&self) -> String {
        match self {
            Self::Message(message) => message.id.clone(),
            Self::Event(event) => {
                format!("event-{:016x}", crate::route::fnv1a(&event_bytes(event)))
            }
        }
    }
}

fn event_bytes(event: &ChannelIncomingEvent) -> Vec<u8> {
    serde_json::to_vec(event).unwrap_or_else(|_error| format!("{event:?}").into_bytes())
}
