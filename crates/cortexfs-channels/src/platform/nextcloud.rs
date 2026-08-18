use std::collections::BTreeMap;

use serde_json::json;

use super::{ChannelCodec, OutboundRequest, object};
use crate::{ChannelError, ChannelId, InboundMessage, OutboundMessage};

mod parse;

/// Nextcloud Talk Bot Activity Streams and legacy webhook codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct NextcloudCodec;

impl ChannelCodec for NextcloudCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("nextcloud_talk")
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        let root = object(payload)?;
        parse::decode(&root)
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        message.body.validate()?;
        if !message.body.attachments.is_empty() {
            return Err(ChannelError::Unsupported(
                "nextcloud talk media attachments".to_owned(),
            ));
        }
        Ok(OutboundRequest {
            method: "POST".to_owned(),
            path: format!(
                "ocs/v2.php/apps/spreed/api/v1/bot/{}/message?format=json",
                message.target.conversation
            ),
            content_type: "application/json".to_owned(),
            body: json!({"message": message.body.text}).to_string(),
            headers: BTreeMap::from([
                ("Accept".to_owned(), "application/json".to_owned()),
                ("OCS-APIRequest".to_owned(), "true".to_owned()),
            ]),
        })
    }
}
