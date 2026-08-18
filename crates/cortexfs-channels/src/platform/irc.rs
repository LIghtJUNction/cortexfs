use super::{ChannelCodec, OutboundRequest};
use crate::{ChannelError, ChannelId, InboundMessage, OutboundMessage};

pub(in crate::platform) mod core;

/// IRC line codec for `PRIVMSG`; TCP connection and nick registration stay host-owned.
#[derive(Clone, Copy, Debug, Default)]
pub struct IrcCodec;

impl ChannelCodec for IrcCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("irc")
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        core::decode(self.channel(), payload)
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        core::encode("irc", message)
    }
}
