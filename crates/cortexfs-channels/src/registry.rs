use std::{collections::BTreeMap, fmt, future::Future, sync::Arc};

use crate::{
    ChannelAdapter, ChannelEffect, ChannelError, ChannelEventStream, ChannelFuture, ChannelHealth,
    ChannelId, ChannelIncoming, ChannelIncomingStream, ChannelStream, DeliveryReceipt,
    InboundMessage, MessageTarget, OutboundMessage,
};

/// Thread-safe collection of named channel adapters.
#[derive(Clone, Default)]
pub struct ChannelRegistry {
    adapters: BTreeMap<ChannelId, Arc<dyn ChannelAdapter>>,
}

impl fmt::Debug for ChannelRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelRegistry")
            .field("ids", &self.ids())
            .finish()
    }
}

impl ChannelRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, adapter: Arc<dyn ChannelAdapter>) -> Result<(), ChannelError> {
        let id = adapter.id();
        if self.adapters.contains_key(&id) {
            return Err(ChannelError::DuplicateChannel(id.to_string()));
        }
        self.adapters.insert(id, adapter);
        Ok(())
    }

    #[must_use]
    pub fn ids(&self) -> Vec<ChannelId> {
        self.adapters.keys().cloned().collect()
    }

    #[must_use]
    pub fn get(&self, id: &ChannelId) -> Option<Arc<dyn ChannelAdapter>> {
        self.adapters.get(id).cloned()
    }

    pub fn listen(&self, id: &ChannelId) -> Result<ChannelStream, ChannelError> {
        self.receive(id)
    }

    pub fn receive(&self, id: &ChannelId) -> Result<ChannelStream, ChannelError> {
        self.get(id)
            .ok_or_else(|| ChannelError::UnknownChannel(id.to_string()))?
            .receive()
    }

    pub fn receive_events(&self, id: &ChannelId) -> Result<ChannelEventStream, ChannelError> {
        self.get(id)
            .ok_or_else(|| ChannelError::UnknownChannel(id.to_string()))?
            .receive_events()
    }

    pub fn receive_incoming(&self, id: &ChannelId) -> Result<ChannelIncomingStream, ChannelError> {
        self.get(id)
            .ok_or_else(|| ChannelError::UnknownChannel(id.to_string()))?
            .receive_incoming()
    }

    #[must_use]
    pub fn start(&self, id: &ChannelId) -> ChannelFuture<'_, ()> {
        let id = id.clone();
        Box::pin(async move {
            self.get(&id)
                .ok_or_else(|| ChannelError::UnknownChannel(id.to_string()))?
                .start()
                .await
        })
    }

    #[must_use]
    pub fn connect(&self, id: &ChannelId) -> ChannelFuture<'_, ()> {
        let id = id.clone();
        Box::pin(async move {
            self.get(&id)
                .ok_or_else(|| ChannelError::UnknownChannel(id.to_string()))?
                .connect()
                .await
        })
    }

    #[must_use]
    pub fn send(&self, message: OutboundMessage) -> ChannelFuture<'_, DeliveryReceipt> {
        Box::pin(async move {
            let id = message.target.channel.clone();
            let adapter = self
                .get(&id)
                .ok_or_else(|| ChannelError::UnknownChannel(id.to_string()))?;
            adapter.send(message).await
        })
    }

    #[must_use]
    pub fn send_effect(
        &self,
        target: MessageTarget,
        effect: ChannelEffect,
    ) -> ChannelFuture<'_, ()> {
        Box::pin(async move {
            let id = target.channel.clone();
            self.get(&id)
                .ok_or_else(|| ChannelError::UnknownChannel(id.to_string()))?
                .send_effect(target, effect)
                .await
        })
    }

    #[must_use]
    pub fn health(&self, id: &ChannelId) -> ChannelFuture<'_, ChannelHealth> {
        let id = id.clone();
        Box::pin(async move {
            self.get(&id)
                .ok_or_else(|| ChannelError::UnknownChannel(id.to_string()))?
                .health()
                .await
        })
    }

    #[must_use]
    pub fn stop(&self, id: &ChannelId) -> ChannelFuture<'_, ()> {
        let id = id.clone();
        Box::pin(async move {
            self.get(&id)
                .ok_or_else(|| ChannelError::UnknownChannel(id.to_string()))?
                .stop()
                .await
        })
    }

    #[must_use]
    pub fn reconnect(&self, id: &ChannelId) -> ChannelFuture<'_, ()> {
        let id = id.clone();
        Box::pin(async move {
            self.get(&id)
                .ok_or_else(|| ChannelError::UnknownChannel(id.to_string()))?
                .reconnect()
                .await
        })
    }

    pub fn dispatch<F, Fut>(
        &self,
        inbound: InboundMessage,
        handler: F,
    ) -> ChannelFuture<'_, DeliveryReceipt>
    where
        F: FnOnce(InboundMessage) -> Fut + Send + 'static,
        Fut: Future<Output = Result<OutboundMessage, ChannelError>> + Send + 'static,
    {
        Box::pin(async move { self.send(handler(inbound).await?).await })
    }

    #[must_use]
    pub fn dispatch_incoming<F, Fut>(
        &self,
        incoming: ChannelIncoming,
        handler: F,
    ) -> ChannelFuture<'_, DeliveryReceipt>
    where
        F: FnOnce(ChannelIncoming) -> Fut + Send + 'static,
        Fut: Future<Output = Result<OutboundMessage, ChannelError>> + Send + 'static,
    {
        Box::pin(async move { self.send(handler(incoming).await?).await })
    }
}
