use crate::{ConversionError, EventStatus, ModelEvent, ToolCall, Usage, WireProtocol};
use serde_json::{Map, Value};

#[derive(Debug)]
pub struct Summary {
    pub run: String,
    pub model: String,
    pub text: String,
    pub calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
    pub status: EventStatus,
}

pub(super) fn summary(
    protocol: WireProtocol,
    events: &[ModelEvent],
) -> Result<Summary, ConversionError> {
    let mut value = Summary {
        run: String::new(),
        model: String::new(),
        text: String::new(),
        calls: Vec::new(),
        usage: None,
        status: EventStatus::Ok,
    };
    for event in events {
        match *event {
            ModelEvent::Start { ref run, ref model } => {
                value.run.clone_from(run);
                value.model.clone_from(model);
            }
            ModelEvent::TextDelta { ref text, .. }
            | ModelEvent::ReasoningDelta { ref text, .. } => {
                value.text.push_str(text);
            }
            ModelEvent::ToolCall { ref call, .. } => value.calls.push(call.clone()),
            ModelEvent::Message { ref message, .. } => {
                value.text.push_str(&message.content.text_value());
                value.calls.extend(message.tool_calls.iter().cloned());
            }
            ModelEvent::Usage { ref usage, .. } => value.usage = Some(usage.clone()),
            ModelEvent::Error { .. } => value.status = EventStatus::Error,
            ModelEvent::Done { status, .. } => value.status = status,
        }
    }
    if value.run.is_empty() || value.model.is_empty() {
        return Err(ConversionError::MissingField {
            protocol,
            field: "start event".to_owned(),
        });
    }
    Ok(value)
}

pub(super) fn parse(protocol: WireProtocol, input: &[u8]) -> Result<Value, ConversionError> {
    serde_json::from_slice(input).map_err(|error| ConversionError::InvalidJson {
        protocol,
        detail: error.to_string(),
    })
}

pub(super) fn object(value: Option<&Value>) -> Option<&Map<String, Value>> {
    value.and_then(Value::as_object)
}

pub(super) fn text(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(ToOwned::to_owned)
}

pub(super) fn number(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64)
}

pub(super) fn usage(map: Option<&Map<String, Value>>) -> Option<Usage> {
    let input = number(
        map.and_then(|value| value.get("input_tokens"))
            .or_else(|| map.and_then(|value| value.get("prompt_tokens"))),
    )?;
    let output = number(
        map.and_then(|value| value.get("output_tokens"))
            .or_else(|| map.and_then(|value| value.get("completion_tokens"))),
    )?;
    Some(Usage {
        input_tokens: input,
        output_tokens: output,
        cached_tokens: None,
        reasoning_tokens: None,
    })
}

pub(super) fn finish(status: EventStatus) -> &'static str {
    match status {
        EventStatus::Ok => "stop",
        EventStatus::Error => "error",
        EventStatus::Cancelled => "cancelled",
    }
}
