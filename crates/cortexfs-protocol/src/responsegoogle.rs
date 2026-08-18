use crate::responsegooglepart::{invalid, missing, usage};
use crate::{ConversionError, EventStatus, ModelEvent, WireProtocol};
use serde_json::{Map, Value, json};

pub(super) fn decode(input: &[u8]) -> Result<Vec<ModelEvent>, ConversionError> {
    let root = crate::responseutil::parse(WireProtocol::Gemini, input)?;
    let map = root.as_object().ok_or_else(|| invalid("response object"))?;
    let run =
        crate::responseutil::text(map.get("responseId")).unwrap_or_else(|| "response".to_owned());
    let model = crate::responseutil::text(map.get("modelVersion"))
        .or_else(|| crate::responseutil::text(map.get("model")))
        .ok_or_else(|| missing("model"))?;
    let mut events = vec![ModelEvent::Start {
        run: run.clone(),
        model,
    }];
    let candidate = map
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_object);
    if let Some(candidate) = candidate {
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
                    events.push(ModelEvent::ToolCall {
                        run: run.clone(),
                        call: crate::ToolCall {
                            id: crate::responseutil::text(call.get("name")).unwrap_or_default(),
                            name: crate::responseutil::text(call.get("name")).unwrap_or_default(),
                            arguments: args,
                        },
                    });
                }
            }
        }
        let status = candidate
            .get("finishReason")
            .and_then(Value::as_str)
            .map_or(EventStatus::Ok, |reason| {
                if reason == "SAFETY" {
                    EventStatus::Error
                } else {
                    EventStatus::Ok
                }
            });
        events.push(ModelEvent::Done {
            run: run.clone(),
            status,
        });
    }
    if let Some(usage) = usage(crate::responseutil::object(map.get("usageMetadata"))) {
        events.push(ModelEvent::Usage {
            run: run.clone(),
            usage,
        });
    }
    if !events
        .iter()
        .any(|event| matches!(event, ModelEvent::Done { .. }))
    {
        events.push(ModelEvent::Done {
            run,
            status: EventStatus::Ok,
        });
    }
    Ok(events)
}

pub(super) fn encode(events: &[ModelEvent]) -> Result<Vec<u8>, ConversionError> {
    let summary = crate::responseutil::summary(WireProtocol::Gemini, events)?;
    let mut parts = vec![json!({"text": summary.text})];
    parts.extend(
        summary
            .calls
            .iter()
            .map(|call| json!({"functionCall": {"name": call.name, "args": call.arguments}})),
    );
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
