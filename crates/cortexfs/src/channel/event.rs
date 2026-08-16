use serde_json::Value;

use super::bridge::ChannelBridgeError;

#[derive(Default)]
pub(crate) struct AssistantEvents {
    final_text: Option<String>,
    deltas: String,
    error: Option<String>,
}

impl AssistantEvents {
    pub(crate) fn push(&mut self, frame: &str) -> Option<String> {
        let Ok(value) = serde_json::from_str::<Value>(frame) else {
            return None;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("message") if value.get("role").and_then(Value::as_str) == Some("assistant") => {
                if let Some(text) = message_text(&value) {
                    self.final_text = Some(text);
                }
            }
            Some("delta") => {
                let text = value.get("text").and_then(Value::as_str)?;
                self.deltas.push_str(text);
                return Some(text.to_owned());
            }
            Some("error") if value.get("recoverable").and_then(Value::as_bool) != Some(true) => {
                self.error = Some(
                    value
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("agent error")
                        .to_owned(),
                );
            }
            Some("done") if value.get("status").and_then(Value::as_str) == Some("error") => {
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

fn message_text(value: &Value) -> Option<String> {
    if let Some(text) = value
        .get("content")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        return Some(text.to_owned());
    }
    let parts = value.get("content").and_then(Value::as_array)?;
    let text = parts
        .iter()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<String>();
    (!text.is_empty()).then_some(text)
}
