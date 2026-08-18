use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{ChannelError, ChannelId, ConversationId};

/// Sender identity supplied by the platform adapter.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Participant {
    pub id: String,
    pub display_name: Option<String>,
    pub handle: Option<String>,
}

/// Text plus optional platform-neutral attachment references.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MessageBody {
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
}

impl MessageBody {
    pub fn text(value: impl Into<String>) -> Result<Self, ChannelError> {
        Self::with_attachments(value, Vec::new())
    }

    pub fn with_attachments(
        text: impl Into<String>,
        attachments: Vec<Attachment>,
    ) -> Result<Self, ChannelError> {
        let body = Self {
            text: text.into(),
            attachments,
        };
        body.validate()?;
        Ok(body)
    }

    pub fn validate(&self) -> Result<(), ChannelError> {
        if self.text.is_empty() && self.attachments.is_empty() {
            return Err(ChannelError::InvalidMessage(
                "message body is empty".to_owned(),
            ));
        }
        if self.text.contains('\0') {
            return Err(ChannelError::InvalidMessage(
                "message text contains NUL".to_owned(),
            ));
        }
        if self
            .attachments
            .iter()
            .any(|attachment| attachment.url.is_empty() || attachment.url.contains('\0'))
        {
            return Err(ChannelError::InvalidMessage(
                "attachment URL is invalid".to_owned(),
            ));
        }
        Ok(())
    }
}

/// A remote attachment represented by a retrievable URL.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    pub url: String,
    pub name: Option<String>,
    pub mime: Option<String>,
}

/// Stable destination identity used by both inbound and outbound messages.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MessageTarget {
    pub channel: ChannelId,
    pub conversation: ConversationId,
    pub thread: Option<String>,
    pub reply_to: Option<String>,
}

/// A message delivered by a channel adapter.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InboundMessage {
    pub id: String,
    pub target: MessageTarget,
    pub sender: Participant,
    pub body: MessageBody,
    pub timestamp_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

/// A message sent through a channel adapter.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutboundMessage {
    pub target: MessageTarget,
    pub body: MessageBody,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}
