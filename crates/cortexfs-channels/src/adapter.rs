use std::future::Future;
use std::pin::Pin;

use futures_core::Stream;

use crate::{
    ChannelCapabilities, ChannelError, ChannelHealth, ChannelId, InboundMessage, OutboundMessage,
};

/// Owned asynchronous inbound stream returned by an adapter.
pub type ChannelStream = Pin<Box<dyn Stream<Item = Result<InboundMessage, ChannelError>> + Send>>;

/// Runtime-neutral future used by object-safe channel adapters.
pub type ChannelFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ChannelError>> + Send + 'a>>;

/// Provider-neutral acknowledgement for one outbound delivery.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeliveryReceipt {
    pub channel: ChannelId,
    pub message_id: String,
    pub target: crate::MessageTarget,
    pub timestamp_ms: Option<u64>,
}

/// Object-safe contract implemented by Telegram, Slack, Discord, or custom adapters.
pub trait ChannelAdapter: Send + Sync {
    fn id(&self) -> ChannelId;
    fn capabilities(&self) -> ChannelCapabilities;
    fn listen(&self) -> Result<ChannelStream, ChannelError>;
    fn send(&self, message: OutboundMessage) -> ChannelFuture<'_, DeliveryReceipt>;
    fn health(&self) -> ChannelFuture<'_, ChannelHealth> {
        Box::pin(async { Ok(ChannelHealth::ready()) })
    }
}
