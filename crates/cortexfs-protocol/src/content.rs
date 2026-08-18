use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Provider-neutral model content.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Content {
    /// Plain text content.
    Text(String),
    /// Ordered multimodal content parts.
    Parts(Vec<ContentPart>),
}

impl Content {
    /// Creates plain text content.
    #[must_use]
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    /// Returns a best-effort text projection for prompt accounting.
    #[must_use]
    pub fn text_value(&self) -> String {
        match *self {
            Self::Text(ref value) => value.clone(),
            Self::Parts(ref parts) => parts
                .iter()
                .filter_map(ContentPart::text_value)
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

/// A content part whose meaning is independent of one provider wire format.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentPart {
    /// Text segment.
    Text { text: String },
    /// Address of an image or other retrievable visual resource.
    Image { uri: String, mime: Option<String> },
    /// Address of an audio resource.
    Audio { uri: String, mime: Option<String> },
    /// Extension data retained by an adapter without entering core semantics.
    Data { name: String, value: Value },
}

impl ContentPart {
    /// Creates a text part.
    #[must_use]
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text { text: value.into() }
    }

    fn text_value(&self) -> Option<String> {
        match *self {
            Self::Text { ref text } => Some(text.clone()),
            _ => None,
        }
    }
}
