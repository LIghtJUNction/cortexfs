use serde_json::json;

use super::{ChannelCodec, OutboundRequest};
use crate::{ChannelCapabilities, ChannelError, ChannelId, OutboundMessage};

/// `WeCom` Bot Webhook codec. The Bot Webhook API is outbound-only.
#[derive(Clone, Copy, Debug, Default)]
pub struct WeComCodec;

impl ChannelCodec for WeComCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("wecom")
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            send: true,
            webhook: true,
            ..ChannelCapabilities::empty()
        }
    }

    fn decode(&self, _payload: &str) -> Result<Option<crate::InboundMessage>, ChannelError> {
        Ok(None)
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        message.body.validate()?;
        if !message.body.attachments.is_empty() {
            return Err(ChannelError::Unsupported(
                "wecom bot media attachments".to_owned(),
            ));
        }
        Ok(OutboundRequest {
            method: "POST".to_owned(),
            path: "send".to_owned(),
            content_type: "application/json".to_owned(),
            body: json!({
                "msgtype": "text",
                "text": {"content": message.body.text},
            })
            .to_string(),
            headers: std::collections::BTreeMap::new(),
        })
    }
}
