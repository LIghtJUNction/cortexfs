use std::io::Read;

use crate::RuntimeClientError;

use super::{
    AGENT_INVOCATION_SCHEMA, AgentInvocationEnvelope, AgentToolObservation,
    MAX_AGENT_CONTEXT_BYTES, MAX_AGENT_INVOCATION_BYTES, MAX_AGENT_STEPS,
};
use crate::interaction::InteractionOrigin;

/// Maximum bytes read for a single invocation frame (payload + trailing newline).
///
/// Keeping this explicit limits allocator growth under malformed peer behavior.
/// Maximum bytes read for one invocation, including its newline.
///
/// Similar protocol framing:
/// - [Rust FS MCP architecture: stdin line-by-line JSON-RPC](https://docs.rs/crate/rust-fs-mcp/0.1.7/source/architecture.md#request-lifecycle)
/// - [modelcontextprotocol/go-sdk discussion on newline transport](https://github.com/orgs/modelcontextprotocol/discussions/364)
const MAX_AGENT_INVOCATION_READ: u64 = 1024 * 1024 + 1;

/// Maximum byte budget for the tool result payload.
///
/// This protects host memory from oversized tool observations.
/// Maximum result bytes carried in one tool observation.
///
/// Truncation strategy parallels:
/// - [rust-fs-mcp truncation metrics](https://docs.rs/crate/rust-fs-mcp/0.1.7/source/architecture.md#l557)
/// - [modelcontextprotocol/servers issue #4206](https://github.com/modelcontextprotocol/servers/issues/4206)
const MAX_OBSERVATION_BYTES: usize = 16 * 1024;
const MAX_EVENT_BYTES: usize = 64 * 1024;

/// Reads and validates one newline-terminated invocation envelope frame from a stream.
///
/// Compatibility strategy:
/// - Read exactly one frame per line, with `read_to_end` plus hard size limits to
///   block sticky frames and oversized payload stalls.
/// - Reject immediately on frame length or newline checks that do not match, returning
///   `InvalidFrame` before protocol state is perturbed.
/// - Validate each field (`schema` / `step` / context limits / observation) as the
///   smallest reusable safety envelope.
///
/// Issue and implementation references:
/// - [modelcontextprotocol/servers#4206](https://github.com/modelcontextprotocol/servers/issues/4206)
/// - [modelcontextprotocol/servers#4207](https://github.com/modelcontextprotocol/servers/issues/4207)
/// - [modelcontextprotocol/servers#3505](https://github.com/modelcontextprotocol/servers/pull/3505)
/// - [modelcontextprotocol/servers#3402](https://github.com/modelcontextprotocol/servers/issues/3402)
/// - [modelcontextprotocol/go-sdk discussion #364](https://github.com/orgs/modelcontextprotocol/discussions/364)
/// - [rust-fs-mcp line based dispatch](https://docs.rs/crate/rust-fs-mcp/0.1.7/source/architecture.md#request-lifecycle)
/// - [CortexFS PR #87](https://github.com/LIghtJUNction/cortexfs/pull/87)
/// - [modelcontextprotocol/python-sdk issue #262](https://github.com/modelcontextprotocol/python-sdk/issues/262)
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

/// Returns whether a tool observation violates the wire contract.
///
/// Validation guardrails:
/// - `tool_call_id` and tool names must not use confusing suffixes.
/// - `status` must be either `ok` or `error`.
/// - Empty names and oversized content are rejected to avoid downstream injection
///   and parse ambiguity.
///
/// Reference implementations:
/// - [modelcontextprotocol/servers pull #4480](https://github.com/modelcontextprotocol/servers/pull/4480)
/// - [CortexFS PR #89](https://github.com/LIghtJUNction/cortexfs/pull/89)
/// - [mark3labs/mcp-filesystem-server main branch](https://github.com/mark3labs/mcp-filesystem-server)
/// - [OpenAI Codex tool_call_id mismatch issue #8479](https://github.com/openai/codex/issues/8479)
/// - [modelcontextprotocol/servers pull #4523](https://github.com/modelcontextprotocol/servers/pull/4523)
/// - [modelcontextprotocol/python-sdk PR #1655](https://github.com/modelcontextprotocol/python-sdk/pull/1655)
fn invalid_observation(value: &AgentToolObservation) -> bool {
    !valid_name(value.tool_call_id())
        || !valid_name(value.name())
        || !matches!(value.status(), "ok" | "error")
        || value.content().len() > MAX_OBSERVATION_BYTES
}

/// Returns whether a tool-call identifier or tool name is canonical.
///
/// This rule follows the MCP filesystem requirement that tool names remain parseable and
/// traceable.
/// - Empty names and overlong names are rejected.
/// - Special characters are forbidden in first position to avoid path and namespace ambiguity.
/// - `.sock` and `.d` suffixes are explicitly rejected to avoid filesystem-style collisions.
///
/// Related implementations:
/// - [rust-fs-mcp tool naming in response and catalog](https://docs.rs/crate/rust-fs-mcp/0.1.7/source/architecture.md#l556)
/// - [Model Context Protocol schema tooling naming fields](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/schema/2025-06-18/schema.ts)
/// - [OpenAI Codex tool call matching issue #2550](https://github.com/microsoft/autogen/issues/2550)
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
