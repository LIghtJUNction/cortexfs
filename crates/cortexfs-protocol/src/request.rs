use crate::{ContextState, Message, ProtocolError, ToolChoice, ToolDefinition};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Versioned `CortexFS` model protocol IR.
pub const MODEL_PROTOCOL: &str = "cortexfs.protocol/v1";

/// One provider-neutral model invocation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    pub protocol: String,
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub context: ContextState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    pub stream: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, Value>,
}

impl ModelRequest {
    /// Creates a non-streaming request with one message.
    #[must_use]
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            protocol: MODEL_PROTOCOL.to_owned(),
            model: model.into(),
            messages,
            context: ContextState::client_owned(),
            tools: Vec::new(),
            tool_choice: None,
            stream: false,
            max_output_tokens: None,
            options: BTreeMap::new(),
        }
    }

    /// Validates the protocol-level invariants before adapter conversion.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.protocol != MODEL_PROTOCOL {
            return Err(ProtocolError::UnsupportedProtocol(self.protocol.clone()));
        }
        if self.model.trim().is_empty() {
            return Err(ProtocolError::EmptyModel);
        }
        if self.messages.is_empty() {
            return Err(ProtocolError::EmptyMessages);
        }
        self.context.validate()?;
        for message in &self.messages {
            if message.role.as_str().trim().is_empty() {
                return Err(ProtocolError::EmptyRole);
            }
        }
        let mut names = Vec::new();
        for tool in &self.tools {
            if tool.name.trim().is_empty() {
                return Err(ProtocolError::EmptyToolName);
            }
            if !tool.parameters.is_object() {
                return Err(ProtocolError::InvalidToolSchema(tool.name.clone()));
            }
            if names.iter().any(|name| name == &tool.name) {
                return Err(ProtocolError::DuplicateTool(tool.name.clone()));
            }
            names.push(tool.name.clone());
        }
        Ok(())
    }

    /// Adds an adapter-neutral option without changing the core schema.
    pub fn option(&mut self, name: impl Into<String>, value: Value) {
        self.options.insert(name.into(), value);
    }
}
