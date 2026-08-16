use std::path::{Path, PathBuf};

use cortexfs_channels::{ChannelError, ChannelSessionRoute, InboundMessage, MessageTarget};
use cortexfs_runtime_client::RuntimeClientError;

mod handle;
mod safe;
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

pub(crate) trait ChannelProgressSink {
    fn begin(&mut self, _inbound: &InboundMessage) {}
    fn delta(&mut self, _text: &str) {}
    fn complete(&mut self, _text: &str) {}
    fn error(&mut self, _message: &str) {}
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
}

impl AgentChannelBridge {
    pub fn new(
        socket: impl Into<PathBuf>,
        route: ChannelSessionRoute,
        cwd: Option<String>,
    ) -> Self {
        Self {
            socket: socket.into(),
            route,
            cwd,
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
}
