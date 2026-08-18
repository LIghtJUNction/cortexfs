use crate::{
    Content, ContentPart, ContextState, Message, ModelRequest, Role, ToolCall, ToolChoice,
    ToolDefinition,
};
use serde_json::Value;
use std::collections::BTreeMap;

/// Zero-copy view over an owned model request.
#[derive(Clone, Copy, Debug)]
pub struct ModelRequestView<'a> {
    pub protocol: &'a str,
    pub model: &'a str,
    pub messages: &'a [Message],
    pub context: &'a ContextState,
    pub tools: &'a [ToolDefinition],
    pub tool_choice: Option<&'a ToolChoice>,
    pub stream: bool,
    pub max_output_tokens: Option<u32>,
    pub options: &'a BTreeMap<String, Value>,
}

/// Zero-copy view over one logical message.
#[derive(Clone, Copy, Debug)]
pub struct MessageView<'a> {
    pub role: &'a str,
    pub content: ContentView<'a>,
    pub name: Option<&'a str>,
    pub tool_call_id: Option<&'a str>,
    pub tool_calls: &'a [ToolCall],
}

/// Zero-copy view over model content and its parts.
#[derive(Clone, Copy, Debug)]
pub enum ContentView<'a> {
    Text(&'a str),
    Parts(&'a [ContentPart]),
}

impl ModelRequest {
    /// Borrows every request field without cloning payload strings or values.
    #[must_use]
    pub fn view(&self) -> ModelRequestView<'_> {
        ModelRequestView {
            protocol: &self.protocol,
            model: &self.model,
            messages: &self.messages,
            context: &self.context,
            tools: &self.tools,
            tool_choice: self.tool_choice.as_ref(),
            stream: self.stream,
            max_output_tokens: self.max_output_tokens,
            options: &self.options,
        }
    }
}

impl Message {
    /// Borrows a message and its nested content without allocating.
    #[must_use]
    pub fn view(&self) -> MessageView<'_> {
        MessageView {
            role: self.role.as_str(),
            content: self.content.view(),
            name: self.name.as_deref(),
            tool_call_id: self.tool_call_id.as_deref(),
            tool_calls: &self.tool_calls,
        }
    }
}

impl Content {
    /// Borrows text or the original parts slice without allocating.
    #[must_use]
    pub fn view(&self) -> ContentView<'_> {
        match *self {
            Self::Text(ref text) => ContentView::Text(text.as_str()),
            Self::Parts(ref parts) => ContentView::Parts(parts.as_slice()),
        }
    }
}

impl Role {
    /// Returns the role without creating an owned string.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}
