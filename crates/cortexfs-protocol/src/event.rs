use crate::{Message, ProviderError, ToolCall, Usage};
use serde::{Deserialize, Serialize};

/// Terminal status of a model invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    Ok,
    Error,
    Cancelled,
}

/// Normalized stream event emitted by every model adapter.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelEvent {
    Start { run: String, model: String },
    TextDelta { run: String, text: String },
    ReasoningDelta { run: String, text: String },
    ToolCall { run: String, call: ToolCall },
    Message { run: String, message: Message },
    Usage { run: String, usage: Usage },
    Error { run: String, error: ProviderError },
    Done { run: String, status: EventStatus },
}

impl ModelEvent {
    /// Returns the run identity carried by any event.
    #[must_use]
    pub fn run(&self) -> &str {
        match *self {
            Self::Start { ref run, .. }
            | Self::TextDelta { ref run, .. }
            | Self::ReasoningDelta { ref run, .. }
            | Self::ToolCall { ref run, .. }
            | Self::Message { ref run, .. }
            | Self::Usage { ref run, .. }
            | Self::Error { ref run, .. }
            | Self::Done { ref run, .. } => run,
        }
    }
}
