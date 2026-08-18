use cortexfs_runtime_client::interaction::InteractionEvent;

use super::bridge::ChannelBridgeError;

#[derive(Default)]
pub(crate) struct AssistantEvents {
    final_text: Option<String>,
    deltas: String,
    error: Option<String>,
}

impl AssistantEvents {
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "match ergonomics keep borrowed interaction fields readable"
    )]
    pub(crate) fn push_interaction(&mut self, event: &InteractionEvent) -> Option<String> {
        match event {
            InteractionEvent::Message { role, text, .. } if role == "assistant" => {
                self.final_text = Some(text.clone());
            }
            InteractionEvent::Delta { text, .. } => {
                self.deltas.push_str(text);
                return Some(text.clone());
            }
            InteractionEvent::Error {
                message, retryable, ..
            } if !retryable => self.error = Some(message.clone()),
            InteractionEvent::Done { status, .. } if status == "error" => {
                self.error = Some("agent run failed".to_owned());
            }
            _ => {}
        }
        None
    }

    pub(crate) fn finish(self) -> Result<String, ChannelBridgeError> {
        if let Some(error) = self.error {
            return Err(ChannelBridgeError::Agent(error));
        }
        self.final_text
            .or_else(|| (!self.deltas.is_empty()).then_some(self.deltas))
            .ok_or(ChannelBridgeError::EmptyReply)
    }
}
