use crate::{Content, ToolCall};
use serde::{Deserialize, Serialize};

/// Common roles are constructors; the string remains open for adapter roles.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Role(pub String);

impl Role {
    /// Creates a role from an adapter-neutral string.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the serialized role name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One logical message in the model IR.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Content,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
}

impl Message {
    /// Creates a message without provider-specific metadata.
    #[must_use]
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::new(role),
            content: Content::text(content),
            name: None,
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    /// Creates a user message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self::new("user", content)
    }

    /// Creates a system message.
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self::new("system", content)
    }

    /// Creates an assistant message.
    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new("assistant", content)
    }
}
