use std::path::{Path, PathBuf};

use cortexfs_channels::{
    ChannelError, ChannelSessionRoute, InboundMessage, MessageBody, MessageTarget, OutboundMessage,
};
use cortexfs_runtime_client::{RuntimeClientError, SessionSendRequest, session};

use super::event::assistant_text;

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

    pub fn handle(&self, inbound: InboundMessage) -> Result<OutboundMessage, ChannelBridgeError> {
        inbound.body.validate()?;
        let session_name = self.route.session_for(&inbound.target);
        let frames = session::send(
            &self.socket,
            SessionSendRequest {
                request_id: &self.route.request_id_for(&inbound),
                session: &session_name,
                scope: "private",
                cwd: self.cwd.as_deref(),
                workspace: None,
                input: &inbound.body.text,
            },
        )?;
        let reply = assistant_text(&frames)?;
        Ok(OutboundMessage {
            target: MessageTarget {
                channel: inbound.target.channel,
                conversation: inbound.target.conversation,
                thread: inbound.target.thread,
                reply_to: Some(inbound.id),
            },
            body: MessageBody::text(reply)?,
            metadata: inbound.metadata,
        })
    }
}
