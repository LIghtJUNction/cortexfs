use crate::{ConversionError, ModelEvent, WireProtocol};
use serde_json::Value;

pub(super) fn text_events(events: &mut Vec<ModelEvent>, run: &str, value: Option<&Value>) {
    if let Some(text) = crate::responseutil::text(value) {
        events.push(ModelEvent::TextDelta {
            run: run.to_owned(),
            text,
        });
    }
    if let Some(parts) = value.and_then(Value::as_array) {
        for part in parts.iter().filter_map(Value::as_object) {
            if let Some(text) = crate::responseutil::text(part.get("text")) {
                events.push(ModelEvent::TextDelta {
                    run: run.to_owned(),
                    text,
                });
            }
        }
    }
}

pub(super) fn tool_call(run: &str, value: &Value) -> Result<ModelEvent, ConversionError> {
    let map = value.as_object().ok_or_else(|| invalid("tool_calls[]"))?;
    let function = crate::responseutil::object(map.get("function"))
        .ok_or_else(|| invalid("tool_calls[].function"))?;
    let required = |value, field| {
        crate::responseutil::text(value)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| invalid(field))
    };
    let arguments = required(function.get("arguments"), "tool_calls[].function.arguments")?;
    let arguments = crate::semantic::json_value(
        WireProtocol::OpenAiChat,
        "tool_calls[].function.arguments",
        &arguments,
    )?;
    Ok(ModelEvent::ToolCall {
        run: run.to_owned(),
        call: crate::ToolCall {
            id: required(map.get("id"), "tool_calls[].id")?,
            name: required(function.get("name"), "tool_calls[].function.name")?,
            arguments,
        },
    })
}

pub(super) fn invalid(field: &str) -> ConversionError {
    ConversionError::InvalidField {
        protocol: WireProtocol::OpenAiChat,
        field: field.to_owned(),
    }
}
