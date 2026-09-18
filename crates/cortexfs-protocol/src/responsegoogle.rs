use crate::responsegooglepart::{invalid, provider_error};
use crate::{ConversionError, EventStatus, ModelEvent, WireProtocol};
use serde_json::{Map, Value, json};

pub(super) fn decode(input: &[u8]) -> Result<Vec<ModelEvent>, ConversionError> {
    let root = crate::responseutil::parse(WireProtocol::Gemini, input)?;
    let map = root.as_object().ok_or_else(|| invalid("response object"))?;
    let run =
        crate::responseutil::text(map.get("responseId")).unwrap_or_else(|| "response".to_owned());
    let error = provider_error(&root);
    let model = crate::responseutil::text(map.get("modelVersion"))
        .or_else(|| crate::responseutil::text(map.get("model")))
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
    let candidate = map
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("candidates"))?;
    if let Some(parts) = candidate
        .get("content")
        .and_then(Value::as_object)
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
    {
        for part in parts {
            if let Some(text) =
                crate::responseutil::text(part.as_object().and_then(|item| item.get("text")))
            {
                events.push(ModelEvent::TextDelta {
                    run: run.clone(),
                    text,
                });
            }
            if let Some(call) = part
                .as_object()
                .and_then(|item| item.get("functionCall"))
                .and_then(Value::as_object)
            {
                let args = call.get("args").cloned().unwrap_or_else(|| json!({}));
                if !args.is_object() {
                    return Err(invalid("functionCall.args"));
                }
                let name = crate::responseutil::text(call.get("name"))
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| invalid("functionCall.name"))?;
                let id = match call.get("id") {
                    None => name.clone(),
                    Some(Value::String(value)) => value.clone(),
                    Some(_) => return Err(invalid("functionCall.id")),
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
        Some("STOP") | None => EventStatus::Ok,
        Some("CANCELLED") => EventStatus::Cancelled,
        Some(_) => EventStatus::Error,
    };
    events.push(ModelEvent::Done {
        run: run.clone(),
        status,
    });
    if let Some(usage) =
        crate::responseutil::usage(crate::responseutil::object(map.get("usageMetadata")))
    {
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
