use serde_json::Value;

use super::{ChannelCodec, OutboundRequest, object};
use crate::{ChannelError, ChannelId, OutboundMessage};

mod parse;

/// Mochat HTTP receive/send codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct MochatCodec;

impl ChannelCodec for MochatCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("mochat")
    }

    fn decode(&self, payload: &str) -> Result<Option<crate::InboundMessage>, ChannelError> {
        let root = object(payload)?;
        let value = root
            .get("data")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .or_else(|| root.get("messages").and_then(Value::as_array)?.first())
            .unwrap_or(&root);
        parse::one(value, self.channel())
    }

    fn decode_many(&self, payload: &str) -> Result<Vec<crate::InboundMessage>, ChannelError> {
        let root = object(payload)?;
        let items = root
            .get("data")
            .or_else(|| root.get("messages"))
            .and_then(Value::as_array);
        items.map_or_else(
            || self.decode(payload).map(|item| item.into_iter().collect()),
            |items| {
                items
                    .iter()
                    .map(|item| parse::one(item, self.channel()))
                    .collect::<Result<Vec<_>, _>>()
                    .map(|items| items.into_iter().flatten().collect())
            },
        )
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        parse::outbound(message)
    }
}
