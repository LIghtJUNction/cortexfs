use serde::{Deserialize, Serialize};

use crate::{ChannelError, ChannelHealth, DeliveryReceipt, InboundMessage, OutboundMessage};

/// Versioned JSON ABI shared by channel hosts and agent software.
pub const CHANNEL_ABI: &str = "cortexfs.channel/v1";

/// One framed event crossing a channel host boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChannelEnvelope {
    pub abi: String,
    pub event: ChannelEvent,
}

impl ChannelEnvelope {
    #[must_use]
    pub fn inbound(message: InboundMessage) -> Self {
        Self::new(ChannelEvent::Inbound(message))
    }

    #[must_use]
    pub fn outbound(message: OutboundMessage) -> Self {
        Self::new(ChannelEvent::Outbound(message))
    }

    #[must_use]
    pub fn receipt(receipt: DeliveryReceipt) -> Self {
        Self::new(ChannelEvent::Receipt(receipt))
    }

    #[must_use]
    pub fn health(health: ChannelHealth) -> Self {
        Self::new(ChannelEvent::Health(health))
    }

    #[must_use]
    pub fn new(event: ChannelEvent) -> Self {
        Self {
            abi: CHANNEL_ABI.to_owned(),
            event,
        }
    }

    pub fn validate(&self) -> Result<(), ChannelError> {
        if self.abi == CHANNEL_ABI {
            Ok(())
        } else {
            Err(ChannelError::Protocol(format!(
                "unsupported channel ABI: {}",
                self.abi
            )))
        }
    }
}

/// Event kinds intentionally remain small; platform details stay in metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ChannelEvent {
    Inbound(InboundMessage),
    Outbound(OutboundMessage),
    Receipt(DeliveryReceipt),
    Health(ChannelHealth),
}
