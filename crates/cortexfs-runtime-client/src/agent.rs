//! Hosted-agent invocation wire contract shared by the runtime SDK.

use crate::interaction::InteractionOrigin;
use serde::{Deserialize, Serialize};

mod wire;

pub use wire::read_agent_invocation;

/// Schema identifier for hosted-agent invocation envelopes.
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

/// Maximum JSON-encoded invocation payload bytes, excluding newline framing.
pub const MAX_AGENT_INVOCATION_BYTES: usize = 1024 * 1024;

/// Maximum bytes retained for each invocation context snapshot.
pub const MAX_AGENT_CONTEXT_BYTES: usize = 64 * 1024;

/// Maximum legal zero-based invocation step, including final completion.
pub const MAX_AGENT_STEPS: u8 = 64;

/// Keeps nullable JSON fields explicit while decoding the envelope shape.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(transparent)]
struct Nullable<T>(Option<T>);

/// Bounded JSON envelope carried by SDK-hosted agents.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentInvocationEnvelope {
    schema: String,
    run: String,
    step: u8,
    input: String,
    #[serde(default)]
    event: Option<serde_json::Value>,
    #[serde(default)]
    origin: Option<InteractionOrigin>,
    history_messages: String,
    tool_context: String,
    observation: Nullable<AgentToolObservation>,
}

/// Tool observation metadata appended between continuation steps.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolObservation {
    tool_call_id: String,
    name: String,
    status: String,
    content: String,
    truncated: bool,
}

macro_rules! string_getters {
    ($(($name:ident, $field:ident, $doc:literal)),+ $(,)?) => {
        $(
            #[doc = $doc]
            #[must_use]
            pub fn $name(&self) -> &str {
                &self.$field
            }
        )+
    };
}

impl AgentInvocationEnvelope {
    string_getters!(
        (schema, schema, "Envelope schema marker."),
        (run, run, "Correlated run identifier."),
        (input, input, "User input text payload."),
        (
            history_messages,
            history_messages,
            "Historical message payload."
        ),
        (tool_context, tool_context, "Tool context payload."),
    );

    /// Current continuation step; zero means the initial invocation.
    #[must_use]
    pub const fn step(&self) -> u8 {
        self.step
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

    /// Optional previous observation for continuation flow.
    #[must_use]
    pub fn observation(&self) -> Option<&AgentToolObservation> {
        self.observation.0.as_ref()
    }
}

impl AgentToolObservation {
    string_getters!(
        (
            tool_call_id,
            tool_call_id,
            "Tool call id for the preceding result."
        ),
        (name, name, "Tool name of the preceding result."),
        (status, status, "Tool call status (`ok` or `error`)."),
        (content, content, "Tool output payload text."),
    );

    /// Whether the tool output was truncated.
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}
