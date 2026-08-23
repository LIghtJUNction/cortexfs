//! Built-in behavior loop helpers for executable agents.
//!
//! The host still owns tool authority and the envelope protocol. These helpers
//! only interpret the `CTX_AGENT_LOOP` behavior hint.

use crate::AgentInvocation;

/// Built-in loop kinds selected by `agent/<name>.d/loop`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuiltinLoop {
    /// One input produces one response.
    Chat,
    /// Alternate model actions and observations.
    React,
    /// Coding-oriented action loop.
    Coding,
    /// Separate planning from execution.
    Planner,
    /// Bounded research steps.
    Research,
    /// Custom loop name from `loop.d/<name>`.
    Custom(String),
}

impl BuiltinLoop {
    /// Returns whether this loop expects tool continuation after a model action.
    #[must_use]
    pub fn allows_tool_continuation(self) -> bool {
        !matches!(self, Self::Chat)
    }
}

/// Parses the configured loop hint from one hosted invocation.
#[must_use]
pub fn parse_builtin_loop(invocation: &AgentInvocation) -> BuiltinLoop {
    match invocation.loop_kind() {
        Some("chat") | None => BuiltinLoop::Chat,
        Some("react") => BuiltinLoop::React,
        Some("coding") => BuiltinLoop::Coding,
        Some("planner") => BuiltinLoop::Planner,
        Some("research") => BuiltinLoop::Research,
        Some(value) => BuiltinLoop::Custom(value.to_owned()),
    }
}
