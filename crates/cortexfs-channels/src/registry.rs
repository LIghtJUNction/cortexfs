use std::{collections::BTreeMap, fmt, future::Future, sync::Arc};

use crate::{
    ChannelAdapter, ChannelError, ChannelFuture, ChannelHealth, ChannelId, ChannelStream,
    DeliveryReceipt, InboundMessage, OutboundMessage,
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
        self.get(id)
            .ok_or_else(|| ChannelError::UnknownChannel(id.to_string()))?
            .listen()
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
    pub fn health(&self, id: &ChannelId) -> ChannelFuture<'_, ChannelHealth> {
        let id = id.clone();
        Box::pin(async move {
            self.get(&id)
                .ok_or_else(|| ChannelError::UnknownChannel(id.to_string()))?
                .health()
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
}
