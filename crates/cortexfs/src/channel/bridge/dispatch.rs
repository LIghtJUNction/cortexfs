use std::cell::RefCell;

use cortexfs_channels::{MessageBody, MessageTarget, OutboundMessage};
use cortexfs_runtime_client::{interaction, session};

use super::{AgentChannelBridge, ChannelBridgeError, ChannelProgressSink, safe};
use crate::channel::event::AssistantEvents;

pub(super) fn run<S: ChannelProgressSink>(
    bridge: &AgentChannelBridge,
    mut target: MessageTarget,
    reply_to: String,
    interaction: interaction::InteractionRequest,
    metadata: std::collections::BTreeMap<String, String>,
    sink: &mut S,
) -> Result<OutboundMessage, ChannelBridgeError> {
    let mut events = AssistantEvents::default();
    let mut deltas = Vec::new();
    let sink = RefCell::new(sink);
    let command = RefCell::new(None);
    let result = session::send_interaction_events_with_commands(
        &bridge.socket,
        interaction,
        |event| {
            if matches!(event, interaction::InteractionEvent::Command { .. }) {
                *command.borrow_mut() = Some(event.clone());
            }
            if let Some(text) = events.push_interaction(&event) {
                deltas.push(text);
            }
            Ok::<(), ChannelBridgeError>(())
        },
        |_event| {
            let Some(event) = command.borrow_mut().take() else {
                return Ok(interaction::InteractionResult::Rejected {
                    reason: "missing interactive command event".to_owned(),
                });
            };
            Ok(sink.borrow_mut().command(&event))
        },
    );
    if let Err(error) = result {
        sink.borrow_mut().error(safe::message(&error));
        return Err(error);
    }
    let reply = match events.finish() {
        Ok(reply) => reply,
        Err(error) => {
            sink.borrow_mut().error(safe::message(&error));
            return Err(error);
        }
    };
    for text in &deltas {
        sink.borrow_mut().delta(text);
    }
    sink.borrow_mut().complete(&reply);
    target.reply_to = Some(reply_to);
    Ok(OutboundMessage {
        target,
        body: MessageBody::text(reply)?,
        metadata,
    })
}
