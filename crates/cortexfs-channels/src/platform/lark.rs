use super::{ChannelCodec, OutboundRequest, feishu::FeishuCodec};
use crate::{ChannelError, ChannelId, InboundMessage, OutboundMessage};

/// Lark alias of the Feishu event and message codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct LarkCodec;

impl ChannelCodec for LarkCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("lark")
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        FeishuCodec.decode(payload).map(|message| {
            message.map(|mut message| {
                message.target.channel = self.channel();
                message
            })
        })
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        FeishuCodec.encode(message)
    }
}
