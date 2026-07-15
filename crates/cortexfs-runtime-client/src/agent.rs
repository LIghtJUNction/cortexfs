//! Hosted-agent invocation wire contract shared by the runtime and SDK.

use serde::Deserialize;

mod wire;
pub use wire::read_agent_invocation;

/// Schema identifier for hosted-agent invocation envelopes.
pub const AGENT_INVOCATION_SCHEMA: &str = "cortexfs.agent-invocation/v1";
/// Required hosted-agent launch ABI.
pub const AGENT_LAUNCH_ABI: &str = "sdk-envelope-v1";
/// Marker argument for the hosted-agent entrypoint.
pub const AGENT_ENVELOPE_ARG: &str = "--cortexfs-sdk-envelope-v1";
/// Maximum encoded invocation bytes, including its newline.
pub const MAX_AGENT_INVOCATION_BYTES: usize = 1024 * 1024;
/// Maximum bytes in history or tool-context text.
pub const MAX_AGENT_CONTEXT_BYTES: usize = 64 * 1024;
/// Maximum continuation step accepted on the wire.
pub const MAX_AGENT_STEPS: u8 = 8;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(transparent)]
struct Nullable<T>(Option<T>);

/// One host-written invocation for an SDK-hosted agent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentInvocationEnvelope {
    schema: String,
    run: String,
    step: u8,
    input: String,
    history_messages: String,
    tool_context: String,
    observation: Nullable<AgentToolObservation>,
}

/// Host-owned result of the preceding tool call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentToolObservation {
    tool_call_id: String,
    name: String,
    status: String,
    content: String,
    truncated: bool,
}

impl AgentInvocationEnvelope {
    /// Returns the run identifier.
    #[must_use]
    pub fn run(&self) -> &str {
        &self.run
    }

    /// Returns the continuation step.
    #[must_use]
    pub const fn step(&self) -> u8 {
        self.step
    }

    /// Returns the original user input.
    #[must_use]
    pub fn input(&self) -> &str {
        &self.input
    }

    /// Returns the bounded history snapshot.
    #[must_use]
    pub fn history_messages(&self) -> &str {
        &self.history_messages
    }

    /// Returns the bounded tool-context snapshot.
    #[must_use]
    pub fn tool_context(&self) -> &str {
        &self.tool_context
    }

    /// Returns the preceding host-owned tool observation.
    #[must_use]
    pub fn observation(&self) -> Option<&AgentToolObservation> {
        self.observation.0.as_ref()
    }
}

impl AgentToolObservation {
    /// Returns the source tool-call identifier.
    #[must_use]
    pub fn tool_call_id(&self) -> &str {
        &self.tool_call_id
    }

    /// Returns the tool name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns `ok` or `error`.
    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Returns the bounded result text.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Returns whether result text was truncated.
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}
