use crate::{ConversionError, EventStatus, ModelEvent, WireProtocol};
use serde_json::{Map, Value, json};

pub(super) fn decode(input: &[u8]) -> Result<Vec<ModelEvent>, ConversionError> {
    let root = crate::responseutil::parse(WireProtocol::Anthropic, input)?;
    let map = root.as_object().ok_or_else(|| invalid("response object"))?;
    let run = crate::responseutil::text(map.get("id")).unwrap_or_else(|| "response".to_owned());
    let model = crate::responseutil::text(map.get("model")).unwrap_or_else(|| "unknown".to_owned());
    let mut events = vec![ModelEvent::Start {
        run: run.clone(),
        model,
    }];
    if let Some(content) = map.get("content").and_then(Value::as_array) {
        for block in content {
            block_events(&mut events, &run, block)?;
        }
    }
    let status = match map.get("stop_reason").and_then(Value::as_str) {
        Some("error") => EventStatus::Error,
        Some("cancelled") => EventStatus::Cancelled,
        _ => EventStatus::Ok,
    };
    if let Some(usage) = crate::responseutil::usage(crate::responseutil::object(map.get("usage"))) {
        events.push(ModelEvent::Usage {
            run: run.clone(),
            usage,
        });
    }
    events.push(ModelEvent::Done { run, status });
    Ok(events)
}

fn block_events(
    events: &mut Vec<ModelEvent>,
    run: &str,
    value: &Value,
) -> Result<(), ConversionError> {
    let map = value.as_object().ok_or_else(|| invalid("content[]"))?;
    match crate::responseutil::text(map.get("type")).as_deref() {
        Some("text") => {
            if let Some(text) = crate::responseutil::text(map.get("text")) {
                events.push(ModelEvent::TextDelta {
                    run: run.to_owned(),
                    text,
                });
            }
        }
        Some("thinking") => {
            if let Some(text) = crate::responseutil::text(map.get("thinking")) {
                events.push(ModelEvent::ReasoningDelta {
                    run: run.to_owned(),
                    text,
                });
            }
        }
        Some("tool_use") => events.push(ModelEvent::ToolCall {
            run: run.to_owned(),
            call: crate::ToolCall {
                id: crate::responseutil::text(map.get("id")).unwrap_or_default(),
                name: crate::responseutil::text(map.get("name")).unwrap_or_default(),
                arguments: map.get("input").cloned().unwrap_or_else(|| json!({})),
            },
        }),
        _ => {}
    }
    Ok(())
}

pub(super) fn encode(events: &[ModelEvent]) -> Result<Vec<u8>, ConversionError> {
    let summary = crate::responseutil::summary(WireProtocol::Anthropic, events)?;
    let mut content = vec![json!({"type": "text", "text": summary.text})];
    content.extend(summary.calls.iter().map(|call| json!({"type": "tool_use", "id": call.id, "name": call.name, "input": call.arguments})));
    let mut root = Map::from_iter([
        (String::from("id"), json!(summary.run)),
        (String::from("model"), json!(summary.model)),
        (String::from("role"), json!("assistant")),
        (String::from("content"), Value::Array(content)),
        (
            String::from("stop_reason"),
            json!(crate::responseutil::finish(summary.status)),
        ),
    ]);
    if let Some(usage) = summary.usage {
        root.insert(
            "usage".to_owned(),
            json!({"input_tokens": usage.input_tokens, "output_tokens": usage.output_tokens}),
        );
    }
    crate::encode::bytes(WireProtocol::Anthropic, &Value::Object(root))
}

fn invalid(field: &str) -> ConversionError {
    ConversionError::InvalidField {
        protocol: WireProtocol::Anthropic,
        field: field.to_owned(),
    }
}
