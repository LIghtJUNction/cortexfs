use crate::{ConversionError, EventStatus, ModelEvent, Usage, WireProtocol};
use serde_json::{Map, Value, json};

pub(super) fn decode(input: &[u8]) -> Result<Vec<ModelEvent>, ConversionError> {
    let root = crate::responseutil::parse(WireProtocol::OpenAiResponses, input)?;
    let map = root.as_object().ok_or_else(|| invalid("response object"))?;
    let run = crate::responseutil::text(map.get("id")).unwrap_or_else(|| "response".to_owned());
    let model = crate::responseutil::text(map.get("model")).unwrap_or_else(|| "unknown".to_owned());
    let mut events = vec![ModelEvent::Start {
        run: run.clone(),
        model,
    }];
    if let Some(output) = map.get("output").and_then(Value::as_array) {
        for item in output {
            output_item(&mut events, &run, item)?;
        }
    }
    if !events
        .iter()
        .any(|event| matches!(event, ModelEvent::TextDelta { .. }))
        && let Some(text) = crate::responseutil::text(map.get("output_text"))
    {
        events.push(ModelEvent::TextDelta {
            run: run.clone(),
            text,
        });
    }
    if let Some(usage) = crate::responseutil::usage(crate::responseutil::object(map.get("usage"))) {
        events.push(ModelEvent::Usage {
            run: run.clone(),
            usage,
        });
    }
    events.push(ModelEvent::Done {
        run,
        status: EventStatus::Ok,
    });
    Ok(events)
}

fn output_item(
    events: &mut Vec<ModelEvent>,
    run: &str,
    value: &Value,
) -> Result<(), ConversionError> {
    let map = value.as_object().ok_or_else(|| invalid("output[]"))?;
    match crate::responseutil::text(map.get("type")).as_deref() {
        Some("message") | None => {
            if let Some(parts) = map.get("content").and_then(Value::as_array) {
                for part in parts {
                    if let Some(text) = crate::responseutil::text(
                        part.as_object().and_then(|item| item.get("text")),
                    )
                    .or_else(|| {
                        crate::responseutil::text(
                            part.as_object().and_then(|item| item.get("refusal")),
                        )
                    }) {
                        events.push(ModelEvent::TextDelta {
                            run: run.to_owned(),
                            text,
                        });
                    }
                }
            }
        }
        Some("function_call") => {
            let arguments = crate::semantic::json_value(
                WireProtocol::OpenAiResponses,
                "output[].arguments",
                crate::responseutil::text(map.get("arguments"))
                    .as_deref()
                    .unwrap_or("{}"),
            )?;
            events.push(ModelEvent::ToolCall {
                run: run.to_owned(),
                call: crate::ToolCall {
                    id: crate::responseutil::text(map.get("call_id")).unwrap_or_default(),
                    name: crate::responseutil::text(map.get("name")).unwrap_or_default(),
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
