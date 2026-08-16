use serde_json::Value;

use super::bridge::ChannelBridgeError;

pub(crate) fn assistant_text(frames: &[String]) -> Result<String, ChannelBridgeError> {
    let mut final_text = None;
    let mut deltas = String::new();
    for frame in frames {
        let Ok(value) = serde_json::from_str::<Value>(frame) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("message") if value.get("role").and_then(Value::as_str) == Some("assistant") => {
                if let Some(text) = message_text(&value) {
                    final_text = Some(text);
                }
            }
            Some("delta") => {
                if let Some(text) = value.get("text").and_then(Value::as_str) {
                    deltas.push_str(text);
                }
            }
            Some("error") => {
                if value.get("recoverable").and_then(Value::as_bool) != Some(true) {
                    let text = value
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("agent error");
                    return Err(ChannelBridgeError::Agent(text.to_owned()));
                }
            }
            Some("done") if value.get("status").and_then(Value::as_str) == Some("error") => {
                return Err(ChannelBridgeError::Agent("agent run failed".to_owned()));
            }
            _ => {}
        }
    }
    final_text
        .or_else(|| (!deltas.is_empty()).then_some(deltas))
        .ok_or(ChannelBridgeError::EmptyReply)
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
