use crate::{ConversionError, EventStatus, ModelEvent, Usage, WireProtocol};
use serde_json::{Map, Value, json};

pub(super) fn decode(input: &[u8]) -> Result<Vec<ModelEvent>, ConversionError> {
    let root = crate::responseutil::parse(WireProtocol::OpenAiResponses, input)?;
    let map = root.as_object().ok_or_else(|| invalid("response object"))?;
    let run = crate::responseutil::text(map.get("id")).ok_or_else(|| invalid("id"))?;
    let mut events = vec![ModelEvent::Start {
        run: run.clone(),
        model: crate::responseutil::text(map.get("model")).ok_or_else(|| invalid("model"))?,
    }];
    if let Some(output) = map.get("output").and_then(Value::as_array) {
        for item in output {
            output_item(&mut events, &run, item)?;
        }
    }
    crate::responseutil::append_output_text_and_usage(&mut events, &run, map);
    events.push(ModelEvent::Done {
        run,
        status: match map.get("status").and_then(Value::as_str) {
            Some("completed") | None => EventStatus::Ok,
            Some("cancelled") => EventStatus::Cancelled,
            Some(_) => EventStatus::Error,
        },
    });
    Ok(events)
}

fn output_item(
    events: &mut Vec<ModelEvent>,
    run: &str,
    value: &Value,
) -> Result<(), ConversionError> {
    let map = value.as_object().ok_or_else(|| invalid("output[]"))?;
    let kind = map
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("output[].type"))?;
    match kind {
        "message" => {
            if let Some(parts) = map.get("content").and_then(Value::as_array) {
                for part in parts {
                    let part = part.as_object();
                    let text = crate::responseutil::text(part.and_then(|x| x.get("text")))
                        .or_else(|| crate::responseutil::text(part.and_then(|x| x.get("refusal"))));
                    if let Some(text) = text {
                        events.push(ModelEvent::TextDelta {
                            run: run.to_owned(),
                            text,
                        });
                    }
                }
            }
        }
        "function_call" => {
            let required = |key| {
                map.get(key)
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid(key))
            };
            let call_id = required("call_id")?;
            let name = required("name")?;
            let arguments = required("arguments")?;
            let arguments = crate::semantic::json_value(
                WireProtocol::OpenAiResponses,
                "output[].arguments",
                arguments,
            )?;
            events.push(ModelEvent::ToolCall {
                run: run.to_owned(),
                call: crate::ToolCall {
                    id: call_id.to_owned(),
                    name: name.to_owned(),
                    arguments,
                },
            });
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn encode(events: &[ModelEvent]) -> Result<Vec<u8>, ConversionError> {
    let summary = crate::responseutil::summary(WireProtocol::OpenAiResponses, events)?;
    let mut output = vec![
        json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": summary.text}]}),
    ];
    output.extend(summary.calls.iter().map(|call| json!({"type": "function_call", "call_id": call.id, "name": call.name, "arguments": call.arguments.to_string()})));
    let mut root = Map::from_iter([
        (String::from("id"), json!(summary.run)),
        (String::from("model"), json!(summary.model)),
        (
            String::from("status"),
            json!(match summary.status {
                EventStatus::Ok => "completed",
                EventStatus::Error => "failed",
                EventStatus::Cancelled => "cancelled",
            }),
        ),
        (String::from("output"), Value::Array(output)),
    ]);
    if let Some(usage) = summary.usage {
        root.insert("usage".to_owned(), usage_value(&usage));
    }
    crate::encode::bytes(WireProtocol::OpenAiResponses, &Value::Object(root))
}

fn usage_value(usage: &Usage) -> Value {
    json!({"input_tokens": usage.input_tokens, "output_tokens": usage.output_tokens, "total_tokens": usage.input_tokens + usage.output_tokens})
}
fn invalid(field: &str) -> ConversionError {
    ConversionError::InvalidField {
        protocol: WireProtocol::OpenAiResponses,
        field: field.to_owned(),
    }
}
