//! Built-in invoke helpers for Tool SDK executables.

use std::env;

/// Host-selected tool invoke mode.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InvokeMode {
    /// Terminal argv/stdin semantics (`CTX_TOOL_MODE=cli`).
    Cli,
    /// Structured Tool SDK JSONL semantics.
    #[default]
    Sdk,
}

impl InvokeMode {
    /// Returns whether argv should be treated as the primary input surface.
    #[must_use]
    pub const fn uses_argv(self) -> bool {
        matches!(self, Self::Cli)
    }
}

/// Reads the invoke mode selected by the host environment.
#[must_use]
pub fn invoke_mode_from_env() -> InvokeMode {
    match env::var("CTX_TOOL_MODE").ok().as_deref() {
        Some("cli") => InvokeMode::Cli,
        _ => InvokeMode::Sdk,
    }
}

/// Returns the configured run id, falling back to a local default.
#[must_use]
pub fn run_id_from_env() -> String {
    env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned())
}

/// Returns the authorized tool object path when supplied by the host.
#[must_use]
pub fn authorized_object_from_env() -> Option<String> {
    env::var("CTX_AUTHORIZED_OBJECT").ok()
}
