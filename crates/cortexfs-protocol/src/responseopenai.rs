use crate::responseopenaipart::{invalid, text_events, tool_call};
use crate::{ConversionError, EventStatus, ModelEvent, Usage, WireProtocol};
use serde_json::{Map, Value, json};

pub(super) fn decode(input: &[u8]) -> Result<Vec<ModelEvent>, ConversionError> {
    let root = crate::responseutil::parse(WireProtocol::OpenAiChat, input)?;
    let map = root.as_object().ok_or_else(|| invalid("response object"))?;
    let run = crate::responseutil::text(map.get("id")).unwrap_or_else(|| "response".to_owned());
    let model = crate::responseutil::text(map.get("model")).unwrap_or_else(|| "unknown".to_owned());
    let mut events = vec![ModelEvent::Start {
        run: run.clone(),
        model,
    }];
    if let Some(choice) = map
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_object)
    {
        if let Some(message) = choice.get("message").and_then(Value::as_object) {
            text_events(&mut events, &run, message.get("content"));
            if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
                for call in calls {
                    events.push(tool_call(&run, call)?);
                }
            }
        }
        let status =
            choice
                .get("finish_reason")
                .and_then(Value::as_str)
                .map_or(EventStatus::Ok, |reason| {
                    if reason == "error" {
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
    crate::responseutil::append_output_text_and_usage(&mut events, &run, map);
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
    let summary = crate::responseutil::summary(WireProtocol::OpenAiChat, events)?;
    let mut root = Map::from_iter([
        (String::from("id"), json!(summary.run)),
        (String::from("model"), json!(summary.model)),
    ]);
    let message = json!({ "role": "assistant", "content": summary.text, "tool_calls": summary.calls.iter().map(|call| json!({"id": call.id, "type": "function", "function": {"name": call.name, "arguments": call.arguments.to_string()}})).collect::<Vec<_>>() });
    root.insert("choices".to_owned(), json!([{"index": 0, "message": message, "finish_reason": crate::responseutil::finish(summary.status)}]));
    if let Some(usage) = summary.usage {
        root.insert("usage".to_owned(), usage_value(&usage));
    }
    crate::encode::bytes(WireProtocol::OpenAiChat, &Value::Object(root))
}

fn usage_value(usage: &Usage) -> Value {
    json!({"prompt_tokens": usage.input_tokens, "completion_tokens": usage.output_tokens, "total_tokens": usage.input_tokens + usage.output_tokens})
}
