use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
/// External transport context kept separate from session semantics.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct InteractionOrigin {
    pub transport: String,
    pub endpoint: Option<String>,
    pub identity: Option<String>,
    pub conversation: Option<String>,
    pub thread: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}
/// Client-to-runtime actions, including replies to runtime prompts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InteractionCommand {
    RequestInput { prompt: String },
    RequestApproval { tool: String, arguments: Value },
    Notify { level: String, text: String },
    Invoke { name: String, payload: Value },
}
/// Result sent after a runtime-initiated command.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InteractionResult {
    Accepted,
    Rejected { reason: String },
    Value { payload: Value },
}
/// Stable operations shared by every `CortexFS` interaction frontend.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[expect(
    clippy::large_enum_variant,
    reason = "input keeps the request ABI inline"
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InteractionRequest {
    Input {
        request_id: String,
        session: String,
        scope: String,
        input: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event: Option<Value>,
        origin: InteractionOrigin,
        cwd: Option<String>,
        workspace: Option<String>,
    },
    Resume {
        request_id: String,
        session: String,
        after: Option<String>,
    },
    Status {
        request_id: String,
        session: String,
    },
    Cancel {
        request_id: String,
        run: String,
    },
    CommandResult {
        request_id: String,
        session: String,
        command_id: String,
        result: InteractionResult,
    },
}
impl InteractionRequest {
    /// Returns the caller-owned correlation id for this request.
    #[must_use]
    pub fn request_id(&self) -> &str {
        match *self {
            Self::Input { ref request_id, .. }
            | Self::Resume { ref request_id, .. }
            | Self::Status { ref request_id, .. }
            | Self::Cancel { ref request_id, .. }
            | Self::CommandResult { ref request_id, .. } => request_id,
        }
    }
    /// Returns the session carried by a session-scoped request.
    #[must_use]
    pub fn session(&self) -> Option<&str> {
        match *self {
            Self::Input { ref session, .. }
            | Self::Resume { ref session, .. }
            | Self::Status { ref session, .. }
            | Self::CommandResult { ref session, .. } => Some(session),
            Self::Cancel { .. } => None,
        }
    }
    #[must_use]
    pub fn input(
        request_id: impl Into<String>,
        session: impl Into<String>,
        input: impl Into<String>,
        origin: InteractionOrigin,
    ) -> Self {
        Self::new_input(request_id, session, input, None, origin)
    }
    /// Creates an input request carrying a provider-neutral external event.
    #[must_use]
    pub fn input_with_event(
        request_id: impl Into<String>,
        session: impl Into<String>,
        input: impl Into<String>,
        event: Value,
        origin: InteractionOrigin,
    ) -> Self {
        Self::new_input(request_id, session, input, Some(event), origin)
    }
    fn new_input(
        request_id: impl Into<String>,
        session: impl Into<String>,
        input: impl Into<String>,
        event: Option<Value>,
        origin: InteractionOrigin,
    ) -> Self {
        Self::Input {
            request_id: request_id.into(),
            session: session.into(),
            scope: "private".to_owned(),
            input: input.into(),
            event,
            origin,
            cwd: None,
            workspace: None,
        }
    }
}
