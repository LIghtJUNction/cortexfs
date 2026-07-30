use std::os::unix::net::UnixStream;

use serde_json::Value;

use super::super::{
    MAX_SOCKET_FRAME_BYTES, read_socket_request_frame_from_stream, write_socket_frame,
};
use super::SocketRuntimeError;
use crate::object::executor::AgentToolCall;

pub(super) struct ToolApproval {
    pub(super) frames: [String; 2],
    pub(super) allowed: bool,
    pub(super) reason: &'static str,
}

pub(super) fn request_tool_approval(
    stream: &mut UnixStream,
    run: &str,
    call: &AgentToolCall,
) -> Result<ToolApproval, SocketRuntimeError> {
    let args = call
        .args
        .iter()
        .map(|arg| arg.to_str().ok_or(SocketRuntimeError::CannotRunAgent))
        .collect::<Result<Vec<_>, _>>()?;
    let request = serde_json::json!({
        "type": "approval_request",
        "run": run,
        "id": call.id,
        "name": call.name,
        "args": args
    })
    .to_string();
    if request.len() > MAX_SOCKET_FRAME_BYTES {
        return Err(SocketRuntimeError::CannotRunAgent);
    }
    let request_delivered = write_socket_frame(stream, &request).is_ok();
    let response = request_delivered
        .then(|| read_socket_request_frame_from_stream(stream))
        .transpose()
        .ok()
        .flatten();
    let decision = response
        .and_then(|line| serde_json::from_str::<Value>(&line).ok())
        .filter(|value| {
            value.as_object().is_some_and(|object| object.len() == 4)
                && value.get("op").and_then(Value::as_str) == Some("approve")
                && value.get("run").and_then(Value::as_str) == Some(run)
                && value.get("id").and_then(Value::as_str) == Some(call.id.as_str())
        })
        .and_then(|value| {
            value
                .get("decision")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    let mut allowed = decision.as_deref() == Some("allow_once");
    let mut reason = if !request_delivered {
        "approval request delivery failed"
    } else if allowed {
        "approved once"
    } else if decision.as_deref() == Some("deny") {
        "tool approval denied"
    } else {
        "invalid or missing tool approval"
    };
    let mut result = approval_result_frame(run, call, allowed, reason);
    if write_socket_frame(stream, &result).is_err() && allowed {
        allowed = false;
        reason = "approval result delivery failed";
        result = approval_result_frame(run, call, false, reason);
    }
    Ok(ToolApproval {
        frames: [request, result],
        allowed,
        reason,
    })
}

fn approval_result_frame(run: &str, call: &AgentToolCall, allowed: bool, reason: &str) -> String {
    serde_json::json!({
        "type": "approval_result",
        "run": run,
        "id": call.id,
        "name": call.name,
        "decision": if allowed { "allow_once" } else { "deny" },
        "reason": reason
    })
    .to_string()
}
