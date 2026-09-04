use std::io::Read;

use crate::RuntimeClientError;

use super::{
    AGENT_INVOCATION_SCHEMA, AgentInvocationEnvelope, AgentToolObservation,
    MAX_AGENT_CONTEXT_BYTES, MAX_AGENT_INVOCATION_BYTES, MAX_AGENT_STEPS,
};
use crate::interaction::InteractionOrigin;

/// Read one byte beyond the frame limit to detect oversized input.
const MAX_AGENT_INVOCATION_READ: u64 = 1024 * 1024 + 1;

/// Maximum result bytes carried in one tool observation.
const MAX_OBSERVATION_BYTES: usize = 16 * 1024;
const MAX_EVENT_BYTES: usize = 64 * 1024;

/// Reads one newline-terminated invocation frame through EOF, within the byte limit.
/// Rejects extra frames, unknown fields, invalid context sizes and observations.
pub fn read_agent_invocation(
    reader: impl Read,
) -> Result<AgentInvocationEnvelope, RuntimeClientError> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_AGENT_INVOCATION_READ)
        .read_to_end(&mut bytes)
        .map_err(|_error| RuntimeClientError::CannotRead)?;

    if bytes.len() > MAX_AGENT_INVOCATION_BYTES
        || bytes.pop() != Some(b'\n')
        || bytes.contains(&b'\n')
    {
        return Err(RuntimeClientError::InvalidFrame);
    }

    let envelope: AgentInvocationEnvelope =
        serde_json::from_slice(&bytes).map_err(|_error| RuntimeClientError::InvalidFrame)?;

    if envelope.schema != AGENT_INVOCATION_SCHEMA
        || envelope.step > MAX_AGENT_STEPS
        || envelope.history_messages.len() > MAX_AGENT_CONTEXT_BYTES
        || envelope.tool_context.len() > MAX_AGENT_CONTEXT_BYTES
        || envelope
            .event
            .as_ref()
            .is_some_and(|event| !event.is_object() || event_bytes_too_large(event))
        || envelope.origin.as_ref().is_some_and(invalid_origin)
        || (envelope.step == 0) != envelope.observation.0.is_none()
        || envelope.observation().is_some_and(invalid_observation)
    {
        return Err(RuntimeClientError::InvalidFrame);
    }

    Ok(envelope)
}

fn event_bytes_too_large(event: &serde_json::Value) -> bool {
    serde_json::to_vec(event).is_ok_and(|bytes| bytes.len() > MAX_EVENT_BYTES)
}

fn invalid_origin(origin: &InteractionOrigin) -> bool {
    serde_json::to_vec(origin).is_ok_and(|bytes| bytes.len() > MAX_EVENT_BYTES)
        || [
            Some(origin.transport.as_str()),
            origin.endpoint.as_deref(),
            origin.identity.as_deref(),
            origin.conversation.as_deref(),
            origin.thread.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|value| value.is_empty() || value.bytes().any(|byte| byte.is_ascii_control()))
        || origin.metadata.iter().any(|(key, value)| {
            key.is_empty()
                || key.bytes().any(|byte| byte.is_ascii_control())
                || value.bytes().any(|byte| byte.is_ascii_control())
        })
}

/// Checks observation identity, completion status and content size.
fn invalid_observation(value: &AgentToolObservation) -> bool {
    !valid_name(value.tool_call_id())
        || !valid_name(value.name())
        || !matches!(value.status(), "ok" | "error")
        || value.content().len() > MAX_OBSERVATION_BYTES
}

/// Accepts canonical tool-call identifiers and tool names up to 255 bytes.
/// Names start with an ASCII letter or digit and cannot end in `.sock` or `.d`.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name.strip_suffix(".sock").is_none()
        && name.strip_suffix(".d").is_none()
        && name.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric()
                || (index != 0 && matches!(byte, b'.' | b'_' | b'+' | b'-'))
        })
}

#[cfg(test)]
mod tests;
