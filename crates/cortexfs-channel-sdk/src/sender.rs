use cortexfs_channels::{
    ChannelDriverError, ChannelDriverSession, ChannelFrameBody, ChannelHealth, ChannelIncoming,
};

/// Cloneable ingress handle for a platform receive loop.
#[derive(Clone, Debug)]
pub struct ChannelSender(ChannelDriverSession);

impl ChannelSender {
    pub(crate) const fn new(session: ChannelDriverSession) -> Self {
        Self(session)
    }

    pub fn send(&self, incoming: ChannelIncoming) -> Result<(), ChannelDriverError> {
        self.0.send_incoming(incoming)
    }

    pub fn heartbeat(&self) -> Result<(), ChannelDriverError> {
        self.0.send_frame(ChannelFrameBody::Event {
            event: cortexfs_channels::ChannelRuntimeEvent::Heartbeat,
        })
    }

    pub fn health(&self, health: ChannelHealth) -> Result<(), ChannelDriverError> {
        self.0.send_frame(ChannelFrameBody::Health { health })
    }
}
