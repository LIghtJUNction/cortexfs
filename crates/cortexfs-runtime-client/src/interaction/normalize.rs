use super::{InteractionCommand, InteractionEvent};
use serde_json::Value;
/// Maps one existing executable-agent event into the shared interaction ABI.
pub fn interaction_event_from_agent_frame(
    request_id: &str,
    frame: &str,
) -> Option<InteractionEvent> {
    let value = serde_json::from_str::<Value>(frame).ok()?;
    let field = |key| value.get(key).and_then(Value::as_str);
    let owned = |key, default| field(key).unwrap_or(default).to_owned();
    let request = || request_id.to_owned();
    let run = owned("run", "");
    match field("type")? {
        "start" => Some(InteractionEvent::Started {
            request_id: request(),
            run,
            model: field("model").map(str::to_owned),
        }),
        "delta" => Some(InteractionEvent::Delta {
            request_id: request(),
            run,
            text: field("text")?.to_owned(),
        }),
        "message" => Some(InteractionEvent::Message {
            request_id: request(),
            run,
            role: owned("role", "unknown"),
            text: message_text(&value)?,
        }),
        "tool_call" => Some(InteractionEvent::Tool {
            request_id: request(),
            run,
            call_id: value
                .get("id")
                .or_else(|| value.get("tool_call_id"))
                .and_then(Value::as_str)?
                .to_owned(),
            name: field("name")?.to_owned(),
            state: "requested".to_owned(),
        }),
        "status" => Some(InteractionEvent::Status {
            request_id: request(),
            session: owned("session", "default"),
            status: owned("status", "unknown"),
            phase: field("phase").map(str::to_owned),
            step: value
                .get("step")
                .and_then(Value::as_u64)
                .and_then(|step| u32::try_from(step).ok())
                .unwrap_or(0),
        }),
        "approval_request" => Some(InteractionEvent::Command {
            request_id: request(),
            run,
            command_id: field("id")?.to_owned(),
            command: InteractionCommand::RequestApproval {
                tool: field("name")?.to_owned(),
                arguments: value.get("args").cloned().unwrap_or(Value::Null),
            },
        }),
        "error" => Some(InteractionEvent::Error {
            request_id: request(),
            run: (!run.is_empty()).then_some(run),
            code: owned("code", "EIO"),
            message: owned("message", "agent error"),
            retryable: value
                .get("recoverable")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }),
        "done" => Some(InteractionEvent::Done {
            request_id: request(),
            run,
            status: owned("status", "unknown"),
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
