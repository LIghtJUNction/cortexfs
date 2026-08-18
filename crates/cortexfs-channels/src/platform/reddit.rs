use serde_json::Value;

use super::{ChannelCodec, OutboundRequest, object};
use crate::{ChannelError, ChannelId, InboundMessage, OutboundMessage};

mod form;
mod parse;

/// Reddit OAuth inbox/comment codec; authentication and polling stay host-owned.
#[derive(Clone, Copy, Debug, Default)]
pub struct RedditCodec;

impl ChannelCodec for RedditCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("reddit")
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        parse::one(&object(payload)?, self.channel())
    }

    fn decode_many(&self, payload: &str) -> Result<Vec<InboundMessage>, ChannelError> {
        let root = object(payload)?;
        let Some(items) = root.pointer("/data/children").and_then(Value::as_array) else {
            return Ok(self.decode(payload)?.into_iter().collect());
        };
        items
            .iter()
            .map(|item| parse::one(item, self.channel()))
            .collect::<Result<Vec<_>, _>>()
            .map(|items| items.into_iter().flatten().collect())
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        parse::outbound(message)
    }
}
