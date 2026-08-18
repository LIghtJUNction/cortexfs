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
        for part in parts {
            if let Some(text) =
                crate::responseutil::text(part.as_object().and_then(|item| item.get("text")))
            {
                events.push(ModelEvent::TextDelta {
                    run: run.to_owned(),
                    text,
                });
            }
        }
    }
}

pub(super) fn tool_call(run: &str, value: &Value) -> Result<ModelEvent, ConversionError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid("choices[].message.tool_calls[]"))?;
    let function = map
        .get("function")
        .and_then(Value::as_object)
        .ok_or_else(|| missing("tool call function"))?;
    let arguments = crate::semantic::json_value(
        WireProtocol::OpenAiChat,
        "tool_calls[].function.arguments",
        crate::responseutil::text(function.get("arguments"))
            .as_deref()
            .unwrap_or("{}"),
    )?;
    Ok(ModelEvent::ToolCall {
        run: run.to_owned(),
        call: crate::ToolCall {
            id: crate::responseutil::text(map.get("id")).unwrap_or_default(),
            name: crate::responseutil::text(function.get("name")).unwrap_or_default(),
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
pub(super) fn missing(field: &str) -> ConversionError {
    ConversionError::MissingField {
        protocol: WireProtocol::OpenAiChat,
        field: field.to_owned(),
    }
}
