use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use cortexfs_channels::{
    ChannelError, ChannelId, ChannelIncoming, ChannelIncomingEvent, ChannelSessionRoute,
    InboundMessage, MessageTarget,
};
use cortexfs_runtime_client::RuntimeClientError;

mod dispatch;
mod event;
mod handle;
mod safe;
mod session;
mod slash;
mod socket;

/// Errors at the boundary between a channel adapter and an agent socket.
#[derive(Debug, thiserror::Error)]
pub enum ChannelBridgeError {
    #[error(transparent)]
    Channel(#[from] ChannelError),
    #[error(transparent)]
    Runtime(#[from] RuntimeClientError),
    #[error("agent returned no assistant message")]
    EmptyReply,
    #[error("agent run failed: {0}")]
    Agent(String),
}

pub trait ChannelProgressSink {
    fn begin(&mut self, _inbound: &InboundMessage) {}
    fn begin_event(&mut self, _target: &MessageTarget) {}
    fn delta(&mut self, _text: &str) {}
    fn complete(&mut self, _text: &str) {}
    fn error(&mut self, _message: &str) {}
    fn command(
        &mut self,
        _event: &cortexfs_runtime_client::interaction::InteractionEvent,
    ) -> cortexfs_runtime_client::interaction::InteractionResult {
        cortexfs_runtime_client::interaction::InteractionResult::Rejected {
            reason: "channel transport has no interactive command reply".to_owned(),
        }
    }
    fn completed(&self) -> bool {
        false
    }
}

impl ChannelProgressSink for () {}

/// Routes every conversation to one durable `CortexFS` agent session.
#[derive(Clone, Debug)]
pub struct AgentChannelBridge {
    socket: PathBuf,
    route: ChannelSessionRoute,
    cwd: Option<String>,
    channel: Option<ChannelId>,
    generations: Arc<Mutex<BTreeMap<String, u32>>>,
}

impl AgentChannelBridge {
    pub fn new(
        socket: impl Into<PathBuf>,
        route: ChannelSessionRoute,
        cwd: Option<String>,
    ) -> Self {
        Self::build(socket, route, cwd, None)
    }

    pub fn new_with_channel(
        socket: impl Into<PathBuf>,
        route: ChannelSessionRoute,
        cwd: Option<String>,
        channel: ChannelId,
    ) -> Self {
        Self::build(socket, route, cwd, Some(channel))
    }

    fn build(
        socket: impl Into<PathBuf>,
        route: ChannelSessionRoute,
        cwd: Option<String>,
        channel: Option<ChannelId>,
    ) -> Self {
        Self {
            socket: socket.into(),
            route,
            cwd,
            channel,
            generations: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub(super) fn bind_message(&self, mut message: InboundMessage) -> InboundMessage {
        if let Some(channel) = self.channel.clone() {
            message.target.channel = channel;
        }
        message
    }

    pub(super) fn bind_event(&self, event: &ChannelIncomingEvent) -> ChannelIncomingEvent {
        self.channel.clone().map_or_else(
            || event.clone(),
            |channel| event.clone().with_channel(channel),
        )
    }

    pub(super) fn bind_incoming(&self, incoming: ChannelIncoming) -> ChannelIncoming {
        if let Some(channel) = self.channel.clone() {
            incoming.with_channel(channel)
        } else {
            incoming
        }
    }

    #[must_use]
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    #[must_use]
    pub fn session_for(&self, target: &MessageTarget) -> String {
        self.route.session_for(target)
    }

    pub fn check_socket(&self) -> Result<(), ChannelBridgeError> {
        socket::check(&self.socket)
    }

    pub fn handle_incoming(
        &self,
        incoming: ChannelIncoming,
    ) -> Result<cortexfs_channels::OutboundMessage, ChannelBridgeError> {
        match self.bind_incoming(incoming) {
            ChannelIncoming::Message(message) => self.handle(message),
            ChannelIncoming::Event(event) => self.handle_event(&event),
        }
    }

    pub fn handle_incoming_with_progress<S: ChannelProgressSink>(
        &self,
        incoming: ChannelIncoming,
        sink: &mut S,
    ) -> Result<cortexfs_channels::OutboundMessage, ChannelBridgeError> {
        match self.bind_incoming(incoming) {
            ChannelIncoming::Message(message) => self.handle_with_progress(message, sink),
            ChannelIncoming::Event(event) => {
                let event_id = self.route.request_id_for_event(&event);
                self.handle_event_with_progress(&event_id, &event, sink)
            }
        }
    }
}
