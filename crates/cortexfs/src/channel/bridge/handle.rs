use cortexfs_channels::{InboundMessage, MessageBody, MessageTarget, OutboundMessage};
use cortexfs_runtime_client::{SessionSendRequest, session};

use super::{AgentChannelBridge, ChannelBridgeError, ChannelProgressSink, safe};
use crate::channel::event::AssistantEvents;

impl AgentChannelBridge {
    pub fn handle(&self, inbound: InboundMessage) -> Result<OutboundMessage, ChannelBridgeError> {
        self.handle_with_progress(inbound, &mut ())
    }

    pub(crate) fn handle_with_progress<S: ChannelProgressSink>(
        &self,
        inbound: InboundMessage,
        sink: &mut S,
    ) -> Result<OutboundMessage, ChannelBridgeError> {
        inbound.body.validate()?;
        sink.begin(&inbound);
        let session_name = self.route.session_for(&inbound.target);
        let mut events = AssistantEvents::default();
        let result = session::send_stream(
            &self.socket,
            SessionSendRequest {
                request_id: &self.route.request_id_for(&inbound),
                session: &session_name,
                scope: "private",
                cwd: self.cwd.as_deref(),
                workspace: None,
                input: &inbound.body.text,
            },
            |frame| {
                if let Some(text) = events.push(frame) {
                    sink.delta(&text);
                }
                Ok::<(), ChannelBridgeError>(())
            },
        );
        if let Err(error) = result {
            sink.error(safe::message(&error));
            return Err(error);
        }
        let reply = match events.finish() {
            Ok(reply) => reply,
            Err(error) => {
                sink.error(safe::message(&error));
                return Err(error);
            }
        };
        sink.complete(&reply);
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
