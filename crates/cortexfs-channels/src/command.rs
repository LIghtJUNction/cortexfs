use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One provider-neutral choice in a channel-presented prompt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChannelChoice {
    pub id: String,
    pub label: String,
}

/// Provider-neutral work requested from a channel-facing client.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChannelCommand {
    RequestInput {
        prompt: String,
    },
    RequestApproval {
        tool: String,
        arguments: Value,
    },
    RequestChoice {
        question: String,
        choices: Vec<ChannelChoice>,
        #[serde(default)]
        multiple: bool,
    },
    Notify {
        level: String,
        text: String,
    },
    Invoke {
        name: String,
        payload: Value,
    },
}

/// Result returned by a channel-facing client for a [`ChannelCommand`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChannelCommandResult {
    Accepted,
    Rejected { reason: String },
    Value { payload: Value },
}
