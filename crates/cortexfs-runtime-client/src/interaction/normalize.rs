use serde_json::Value;

use super::InteractionEvent;

/// Maps one existing executable-agent event into the shared interaction ABI.
pub fn interaction_event_from_agent_frame(
    request_id: &str,
    frame: &str,
) -> Option<InteractionEvent> {
    let value = serde_json::from_str::<Value>(frame).ok()?;
    let run = value
        .get("run")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    match value.get("type").and_then(Value::as_str)? {
        "start" => Some(InteractionEvent::Started {
            request_id: request_id.to_owned(),
            run,
            model: value
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }),
        "delta" => Some(InteractionEvent::Delta {
            request_id: request_id.to_owned(),
            run,
            text: value.get("text").and_then(Value::as_str)?.to_owned(),
        }),
        "message" => Some(InteractionEvent::Message {
            request_id: request_id.to_owned(),
            run,
            role: value
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            text: message_text(&value)?,
        }),
        "tool_call" => Some(InteractionEvent::Tool {
            request_id: request_id.to_owned(),
            run,
            call_id: value
                .get("id")
                .or_else(|| value.get("tool_call_id"))
                .and_then(Value::as_str)?
                .to_owned(),
            name: value.get("name").and_then(Value::as_str)?.to_owned(),
            state: "requested".to_owned(),
        }),
        "status" => Some(InteractionEvent::Status {
            request_id: request_id.to_owned(),
            session: value
                .get("session")
                .and_then(Value::as_str)
                .unwrap_or("default")
                .to_owned(),
            status: value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            phase: value
                .get("phase")
                .and_then(Value::as_str)
                .map(str::to_owned),
            step: value
                .get("step")
                .and_then(Value::as_u64)
                .and_then(|step| u32::try_from(step).ok())
                .unwrap_or(0),
        }),
        "approval_request" => Some(InteractionEvent::Command {
            request_id: request_id.to_owned(),
            run,
            command_id: value.get("id").and_then(Value::as_str)?.to_owned(),
            command: super::InteractionCommand::RequestApproval {
                tool: value.get("name").and_then(Value::as_str)?.to_owned(),
                arguments: value.get("args").cloned().unwrap_or(Value::Null),
            },
        }),
        "error" => Some(InteractionEvent::Error {
            request_id: request_id.to_owned(),
            run: (!run.is_empty()).then_some(run),
            code: value
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("EIO")
                .to_owned(),
            message: value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("agent error")
                .to_owned(),
            retryable: value
                .get("recoverable")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }),
        "done" => Some(InteractionEvent::Done {
            request_id: request_id.to_owned(),
            run,
            status: value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
        }),
        _ => None,
    }
}

fn message_text(value: &Value) -> Option<String> {
    value
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            value
                .get("content")
                .and_then(Value::as_array)
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(|part| part.get("text").and_then(Value::as_str))
                        .collect()
                })
                .filter(|text: &String| !text.is_empty())
        })
}
