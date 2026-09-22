use crate::responsegooglepart::{invalid, provider_error};
use crate::responseutil::{object, text, usage};
use crate::{ConversionError, EventStatus, ModelEvent, WireProtocol};
use serde_json::{Map, Value, json};

pub(super) fn decode(input: &[u8]) -> Result<Vec<ModelEvent>, ConversionError> {
    let root = crate::responseutil::parse(WireProtocol::Gemini, input)?;
    let map = root.as_object().ok_or_else(|| invalid("response object"))?;
    let run = text(map.get("responseId")).unwrap_or_else(|| "response".to_owned());
    let error = provider_error(&root);
    let model = text(map.get("modelVersion").or_else(|| map.get("model")))
        .or_else(|| error.as_ref().map(|_| "unknown".to_owned()))
        .ok_or_else(|| ConversionError::MissingField {
            protocol: WireProtocol::Gemini,
            field: "model".to_owned(),
        })?;
    let mut events = vec![ModelEvent::Start {
        run: run.clone(),
        model,
    }];
    if let Some(error) = error {
        events.push(ModelEvent::Error {
            run: run.clone(),
            error,
        });
        events.push(ModelEvent::Done {
            run,
            status: EventStatus::Error,
        });
        return Ok(events);
    }
    let candidate = match map.get("candidates").and_then(Value::as_array) {
        Some(items) if items.len() == 1 => &items[0],
        _ => return Err(invalid("candidates")),
    };
    if let Some(parts) = candidate
        .pointer("/content/parts")
        .and_then(Value::as_array)
    {
        for (index, part) in parts.iter().enumerate() {
            if let Some(text) = text(part.get("text")) {
                events.push(ModelEvent::TextDelta {
                    run: run.clone(),
                    text,
                });
            }
            if let Some(call) = part.get("functionCall").and_then(Value::as_object) {
                let args = call.get("args").cloned().unwrap_or_else(|| json!({}));
                args.as_object()
                    .ok_or_else(|| invalid("functionCall.args"))?;
                let name = text(call.get("name"))
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| invalid("functionCall.name"))?;
                let id = match call.get("id") {
                    None => crate::gemini::correlation_id(None, index),
                    Some(value) => text(Some(value)).ok_or_else(|| invalid("functionCall.id"))?,
                };
                events.push(ModelEvent::ToolCall {
                    run: run.clone(),
                    call: crate::ToolCall {
                        id,
                        name,
                        arguments: args,
                    },
                });
            }
        }
    }
    let status = match candidate.get("finishReason").and_then(Value::as_str) {
        Some("STOP") => EventStatus::Ok,
        Some("CANCELLED") => EventStatus::Cancelled,
        Some(_) => EventStatus::Error,
        None => return Err(invalid("finishReason")),
    };
    events.push(ModelEvent::Done {
        run: run.clone(),
        status,
    });
    if let Some(usage) = usage(object(map.get("usageMetadata"))) {
        events.push(ModelEvent::Usage { run, usage });
    }
    Ok(events)
}

pub(super) fn encode(events: &[ModelEvent]) -> Result<Vec<u8>, ConversionError> {
    let summary = crate::responseutil::summary(WireProtocol::Gemini, events)?;
    let mut parts = vec![json!({"text": summary.text})];
    parts.extend(summary.calls.iter().map(
        |call| json!({"functionCall": {"id": call.id, "name": call.name, "args": call.arguments}}),
    ));
    let mut root = Map::from_iter([
        (String::from("responseId"), json!(summary.run)),
        (String::from("modelVersion"), json!(summary.model)),
        (
            String::from("candidates"),
            json!([{"content": {"role": "model", "parts": parts}, "finishReason": crate::responseutil::finish(summary.status).to_uppercase()}]),
        ),
    ]);
    if let Some(usage) = summary.usage {
        root.insert("usageMetadata".to_owned(), json!({"promptTokenCount": usage.input_tokens, "candidatesTokenCount": usage.output_tokens, "totalTokenCount": usage.input_tokens + usage.output_tokens}));
    }
    crate::encode::bytes(WireProtocol::Gemini, &Value::Object(root))
}
