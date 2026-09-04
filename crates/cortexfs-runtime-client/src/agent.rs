//! Hosted-agent invocation wire contract shared by runtime SDK.

use crate::interaction::InteractionOrigin;
use serde::{Deserialize, Serialize};

mod wire;

pub use wire::read_agent_invocation;

/// Schema identifier for hosted-agent invocation envelopes.
///
/// This string is checked for inbound envelope compatibility before dispatch,
/// preventing implicit fallback to legacy ad-hoc command payloads.
pub const AGENT_INVOCATION_SCHEMA: &str = "cortexfs.agent-invocation/v1";

/// Marker selecting the ABI for SDK-hosted agent launches.
pub const AGENT_LAUNCH_ABI: &str = "sdk-envelope-v1";

/// Returns whether `value` is the launch ABI, with one optional trailing newline.
#[must_use]
pub fn is_agent_launch_abi(value: &str) -> bool {
    value == AGENT_LAUNCH_ABI || value.strip_suffix('\n') == Some(AGENT_LAUNCH_ABI)
}

/// Entry argument shared by host and envelope-mode child processes.
pub const AGENT_ENVELOPE_ARG: &str = "--cortexfs-sdk-envelope-v1";

/// Maximum JSON-encoded invocation frame bytes, including its trailing newline.
pub const MAX_AGENT_INVOCATION_BYTES: usize = 1024 * 1024;

/// Maximum bytes retained for history/context snapshots in an invocation payload.
pub const MAX_AGENT_CONTEXT_BYTES: usize = 64 * 1024;

/// Maximum legal zero-based invocation step in one hosted-agent run.
///
/// This includes the final assistant-completion step and is not a tool-call
/// count. The host may invoke every step from zero through this value; after
/// the final step, no further tool continuation is allowed.
pub const MAX_AGENT_STEPS: u8 = 64;

/// Nullable container that keeps `serde_json`-compatible optional fields
/// explicit while preserving envelope-shape invariants during decoding.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(transparent)]
struct Nullable<T>(Option<T>);

/// Envelope payload carried by SDK-hosted agents.
///
/// Each invocation is a single JSON object with bounded fields for compatibility
/// with socket framed transports.
///
/// Behavioral and source references:
/// - [RFC for MCP request identifiers](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/schema/2025-06-18/schema.ts)
/// - [rust-fs-mcp request lifecycle](https://docs.rs/crate/rust-fs-mcp/0.1.7/source/architecture.md#request-lifecycle)
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentInvocationEnvelope {
    /// Envelope schema marker.
    schema: String,
    /// Correlated run identifier shared through multi-step invocations.
    run: String,
    /// Current continuation step.
    step: u8,
    /// User input for the current invocation.
    input: String,
    /// Optional provider-neutral external event that caused the invocation.
    #[serde(default)]
    event: Option<serde_json::Value>,
    /// Provider-neutral external transport origin for channel runs.
    #[serde(default)]
    origin: Option<InteractionOrigin>,
    /// History context snapshot for this invocation.
    history_messages: String,
    /// Tool context snapshot for this invocation.
    tool_context: String,
    /// Previous tool observation for continuation steps.
    observation: Nullable<AgentToolObservation>,
}

/// Tool observation metadata appended between continuation steps.
///
/// Validation logic reference:
/// - [modelcontextprotocol/servers issue #4207](https://github.com/modelcontextprotocol/servers/issues/4207)
/// - [CortexFS PR #87](https://github.com/LIghtJUNction/cortexfs/pull/87)
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolObservation {
    /// Tool call id for correlation with downstream tool state.
    tool_call_id: String,
    /// Tool name reported by the runtime.
    name: String,
    /// Status of the prior tool call (`ok` or `error`).
    status: String,
    /// Text content returned by the tool.
    content: String,
    /// Whether the returned content was truncated by upstream limits.
    truncated: bool,
}

impl AgentInvocationEnvelope {
    /// Envelope schema marker.
    ///
    /// - [CortexFS PR #88](https://github.com/LIghtJUNction/cortexfs/pull/88)
    /// - [modelcontextprotocol/servers PR #4480](https://github.com/modelcontextprotocol/servers/pull/4480)
    #[must_use]
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// Correlated run identifier.
    ///
    /// - [MCP JSON-RPC request id semantics](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/schema/2025-06-18/schema.ts)
    #[must_use]
    pub fn run(&self) -> &str {
        &self.run
    }

    /// Current continuation step (zero means initial invocation).
    ///
    /// Similar handling:
    /// - [deepwiki rust-sdk request/response patterns](https://deepwiki.com/modelcontextprotocol/rust-sdk/7-advanced-topics)
    #[must_use]
    pub const fn step(&self) -> u8 {
        self.step
    }

    /// User input text payload.
    ///
    /// - [CortexFS PR #89](https://github.com/LIghtJUNction/cortexfs/pull/89)
    #[must_use]
    pub fn input(&self) -> &str {
        &self.input
    }

    /// Structured external event, when this invocation came from a channel.
    #[must_use]
    pub fn event(&self) -> Option<&serde_json::Value> {
        self.event.as_ref()
    }

    /// External transport origin, when this invocation came from a channel.
    #[must_use]
    pub fn origin(&self) -> Option<&InteractionOrigin> {
        self.origin.as_ref()
    }

    /// Historical message payload carried in this invocation.
    ///
    /// - [rust-fs-mcp tool result truncation model](https://docs.rs/crate/rust-fs-mcp/0.1.7/source/architecture.md#l557)
    #[must_use]
    pub fn history_messages(&self) -> &str {
        &self.history_messages
    }

    /// Tool context payload carried in this invocation.
    ///
    /// - [modelcontextprotocol/rust-sdk issue #455](https://github.com/modelcontextprotocol/rust-sdk/issues/455)
    #[must_use]
    pub fn tool_context(&self) -> &str {
        &self.tool_context
    }

    /// Optional previous observation for continuation flow.
    ///
    /// - [modelcontextprotocol/servers#4206](https://github.com/modelcontextprotocol/servers/issues/4206)
    #[must_use]
    pub fn observation(&self) -> Option<&AgentToolObservation> {
        self.observation.0.as_ref()
    }
}

impl AgentToolObservation {
    /// Tool call id for the preceding tool result.
    ///
    /// - [CortexFS PR #87](https://github.com/LIghtJUNction/cortexfs/pull/87)
    #[must_use]
    pub fn tool_call_id(&self) -> &str {
        &self.tool_call_id
    }

    /// Tool name of the preceding tool result.
    ///
    /// - [MCP filesystem file naming safety patterns](https://github.com/modelcontextprotocol/servers/issues/4207)
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Tool call status (`ok` or `error`).
    ///
    /// - [modelcontextprotocol/servers issue #4206](https://github.com/modelcontextprotocol/servers/issues/4206)
    /// - [CortexFS PR #89](https://github.com/LIghtJUNction/cortexfs/pull/89)
    /// - [modelcontextprotocol/servers pull #4480](https://github.com/modelcontextprotocol/servers/pull/4480)
    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Tool output payload text.
    ///
    /// - [modelcontextprotocol/servers issue #4207](https://github.com/modelcontextprotocol/servers/issues/4207)
    /// - [rust-fs-mcp request lifecycle](https://docs.rs/crate/rust-fs-mcp/0.1.7/source/architecture.md#request-lifecycle)
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Whether the tool output was truncated.
    ///
    /// - [rust-fs-mcp truncation flag](https://docs.rs/crate/rust-fs-mcp/0.1.7/source/architecture.md#l557)
    /// - [modelcontextprotocol/servers issue #4206](https://github.com/modelcontextprotocol/servers/issues/4206)
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}
