use std::future::Future;
use std::pin::Pin;

use futures_core::Stream;

use crate::{
    ChannelActions, ChannelCapabilities, ChannelEffect, ChannelError, ChannelHealth, ChannelId,
    ChannelIncoming, InboundMessage, MessageTarget, OutboundMessage,
};

/// Owned asynchronous inbound stream returned by an adapter.
pub type ChannelStream = Pin<Box<dyn Stream<Item = Result<InboundMessage, ChannelError>> + Send>>;

/// Receive stream that can carry messages and non-message channel events.
pub type ChannelEventStream =
    Pin<Box<dyn Stream<Item = Result<ChannelIncoming, ChannelError>> + Send>>;

/// Primary receive stream for a channel adapter.
pub type ChannelIncomingStream = ChannelEventStream;

/// Runtime-neutral future used by object-safe channel adapters.
pub type ChannelFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ChannelError>> + Send + 'a>>;

/// Provider-neutral acknowledgement for one outbound delivery.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeliveryReceipt {
    pub channel: ChannelId,
    pub message_id: String,
    pub target: MessageTarget,
    pub timestamp_ms: Option<u64>,
}

/// Object-safe contract implemented by Telegram, Slack, Discord, or custom adapters.
pub trait ChannelAdapter: Send + Sync {
    fn id(&self) -> ChannelId;
    fn capabilities(&self) -> ChannelCapabilities;
    fn actions(&self) -> ChannelActions {
        ChannelActions::empty()
    }
    fn connect(&self) -> ChannelFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
    fn start(&self) -> ChannelFuture<'_, ()> {
        self.connect()
    }
    fn receive(&self) -> Result<ChannelStream, ChannelError> {
        self.listen()
    }
    fn listen(&self) -> Result<ChannelStream, ChannelError>;
    fn receive_events(&self) -> Result<ChannelEventStream, ChannelError> {
        Err(ChannelError::Unsupported(
            "channel does not expose an event stream".to_owned(),
        ))
    }
    /// Returns one stream for both messages and non-message events.
    ///
    /// Existing adapters that only implement [`Self::listen`] are lifted into
    /// the unified stream automatically. Adapters with a native event stream
    /// can keep overriding [`Self::receive_events`].
    fn receive_incoming(&self) -> Result<ChannelIncomingStream, ChannelError> {
        match self.receive_events() {
            Ok(stream) => Ok(stream),
            Err(ChannelError::Unsupported(_)) => {
                Ok(Box::pin(MessageIncomingStream(self.listen()?)))
            }
            Err(error) => Err(error),
        }
    }
    fn send(&self, message: OutboundMessage) -> ChannelFuture<'_, DeliveryReceipt>;
    fn send_effect(&self, _target: MessageTarget, _effect: ChannelEffect) -> ChannelFuture<'_, ()> {
        Box::pin(async {
            Err(ChannelError::Unsupported(
                "channel does not apply live effects".to_owned(),
            ))
        })
    }
    fn stop(&self) -> ChannelFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
    fn reconnect(&self) -> ChannelFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
    fn health(&self) -> ChannelFuture<'_, ChannelHealth> {
        Box::pin(async { Ok(ChannelHealth::ready()) })
    }
}

struct MessageIncomingStream(ChannelStream);

impl Stream for MessageIncomingStream {
    type Item = Result<ChannelIncoming, ChannelError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.0
            .as_mut()
            .poll_next(cx)
            .map(|item| item.map(|result| result.map(ChannelIncoming::Message)))
    }
}
