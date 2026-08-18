use cortexfs_channels::{InboundMessage, OutboundMessage};
use cortexfs_runtime_client::interaction;
use serde_json::{Value, json};

use super::{AgentChannelBridge, ChannelBridgeError, ChannelProgressSink, dispatch};

impl AgentChannelBridge {
    pub fn handle(&self, inbound: InboundMessage) -> Result<OutboundMessage, ChannelBridgeError> {
        self.handle_with_progress(inbound, &mut ())
    }

    pub(crate) fn handle_with_progress<S: ChannelProgressSink>(
        &self,
        inbound: InboundMessage,
        sink: &mut S,
    ) -> Result<OutboundMessage, ChannelBridgeError> {
        let inbound = self.bind_message(inbound);
        inbound.body.validate()?;
        sink.begin(&inbound);
        let interaction = interaction::InteractionRequest::Input {
            request_id: self.route.request_id_for(&inbound),
            session: self.route.session_for_message(&inbound),
            scope: "private".to_owned(),
            input: inbound.body.text.clone(),
            event: attachment_event(&inbound),
            origin: interaction::InteractionOrigin {
                transport: "channel".to_owned(),
                endpoint: Some(inbound.target.channel.to_string()),
                identity: Some(inbound.sender.id.clone()),
                conversation: Some(inbound.target.conversation.to_string()),
                thread: inbound.target.thread.clone(),
                metadata: inbound.metadata.clone(),
            },
            cwd: self.cwd.clone(),
            workspace: None,
        };
        dispatch::run(
            self,
            inbound.target,
            inbound.id,
            interaction,
            inbound.metadata,
            sink,
        )
    }
}

fn attachment_event(inbound: &InboundMessage) -> Option<Value> {
    (!inbound.body.attachments.is_empty()).then(|| {
        json!({
            "type": "message",
            "attachments": &inbound.body.attachments,
        })
    })
}
