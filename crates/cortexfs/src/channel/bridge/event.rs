use cortexfs_channels::{ChannelIncomingEvent, OutboundMessage};
use cortexfs_runtime_client::interaction;

use super::{AgentChannelBridge, ChannelBridgeError, ChannelProgressSink, dispatch};

impl AgentChannelBridge {
    pub fn handle_event(
        &self,
        event: &ChannelIncomingEvent,
    ) -> Result<OutboundMessage, ChannelBridgeError> {
        let event = self.bind_event(event);
        let event_id = self.route.request_id_for_event(&event);
        self.handle_event_with_progress(&event_id, &event, &mut ())
    }

    pub(crate) fn handle_event_with_progress<S: ChannelProgressSink>(
        &self,
        event_id: &str,
        event: &ChannelIncomingEvent,
        sink: &mut S,
    ) -> Result<OutboundMessage, ChannelBridgeError> {
        let event = self.bind_event(event);
        let context = event.context();
        self.route
            .authorize_sender(context.participant.as_ref().map(|actor| actor.id.as_str()))?;
        event.validate()?;
        sink.begin_event(&context.target);
        let value = serde_json::to_value(&event).map_err(|_error| {
            ChannelBridgeError::Channel(cortexfs_channels::ChannelError::Protocol(
                "cannot encode channel event".to_owned(),
            ))
        })?;
        let input = format!("External channel event:\n{value}");
        let interaction = interaction::InteractionRequest::Input {
            request_id: event_id.to_owned(),
            session: self.route.session_for_event(&event),
            scope: "private".to_owned(),
            input,
            event: Some(value),
            origin: interaction::InteractionOrigin {
                transport: "channel".to_owned(),
                endpoint: Some(context.target.channel.to_string()),
                identity: context.participant.as_ref().map(|item| item.id.clone()),
                conversation: Some(context.target.conversation.to_string()),
                thread: context.target.thread.clone(),
                metadata: context.metadata.clone(),
            },
            cwd: self.cwd.clone(),
            workspace: None,
        };
        let reply_to = event
            .message_id()
            .map_or_else(|| event_id.to_owned(), str::to_owned);
        dispatch::run(
            self,
            context.target.clone(),
            reply_to,
            interaction,
            context.metadata.clone(),
            sink,
        )
    }
}
