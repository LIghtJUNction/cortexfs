use serde::{Deserialize, Serialize};

/// Whether a provider capability is known at catalog time.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    Supported,
    Unsupported,
    #[default]
    Unknown,
}

/// Input or output modality understood by a model.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    Text,
    Image,
    Audio,
    Video,
    Pdf,
    Embedding,
}

/// Lifecycle state recorded by the catalog.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStatus {
    Active,
    Preview,
    Deprecated,
}

/// Provider-specific reasoning controls normalized to levels.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReasoningMetadata {
    pub support: Support,
    pub levels: Vec<String>,
    pub parameter: Option<String>,
    pub default_level: Option<String>,
    pub max_tokens: Option<u32>,
}
