use crate::{ConversionError, EventStatus, ModelEvent, Usage, WireProtocol};
use serde_json::{Value, json};

pub(super) fn decode(input: &[u8]) -> Result<Vec<ModelEvent>, ConversionError> {
    let root = crate::responseutil::parse(WireProtocol::Anthropic, input)?;
    let map = root.as_object().ok_or_else(|| invalid("response object"))?;
    let run = crate::responseutil::text(map.get("id")).ok_or_else(|| invalid("id"))?;
    let mut events = vec![ModelEvent::Start {
        run: run.clone(),
        model: crate::responseutil::text(map.get("model")).ok_or_else(|| invalid("model"))?,
    }];
    let content = map.get("content").and_then(Value::as_array);
    for block in content.into_iter().flatten() {
        block_events(&mut events, &run, block)?;
    }
    let status = match map.get("stop_reason").and_then(Value::as_str) {
        Some("end_turn" | "stop_sequence" | "tool_use" | "stop") => EventStatus::Ok,
        Some("cancelled") => EventStatus::Cancelled,
        Some(_) => EventStatus::Error,
        None => return Err(invalid("stop_reason")),
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
                id: crate::responseutil::text(map.get("id")).ok_or_else(|| invalid("id"))?,
                name: crate::responseutil::text(map.get("name")).ok_or_else(|| invalid("name"))?,
                arguments: match map.get("input") {
                    Some(input) if input.is_object() => input.clone(),
                    _ => return Err(invalid("input")),
                },
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
    let mut root = json!({"id": summary.run, "model": summary.model, "role": "assistant", "content": content, "stop_reason": crate::responseutil::finish(summary.status)});
    let usage =
        |u: Usage| json!({"input_tokens": u.input_tokens, "output_tokens": u.output_tokens});
    if let (Some(u), Some(root)) = (summary.usage, root.as_object_mut()) {
        root.insert("usage".into(), usage(u));
    }
    crate::encode::bytes(WireProtocol::Anthropic, &root)
}

fn invalid(field: &str) -> ConversionError {
    ConversionError::InvalidField {
        protocol: WireProtocol::Anthropic,
        field: field.to_owned(),
    }
}
