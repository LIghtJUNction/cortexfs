use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One normalized history message used by the context builder.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Message {
    role: String,
    content: String,
}

impl Message {
    /// Creates a normalized message from a role and text body.
    #[must_use]
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }

    /// Returns the wire role without interpreting provider-specific roles.
    #[must_use]
    pub fn role(&self) -> &str {
        &self.role
    }

    /// Returns the normalized text body.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
}

/// Reads the role and textual content from one durable JSONL message.
#[must_use]
pub fn message_from_json_line(line: &str) -> Option<Message> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    let role = value.get("role")?.as_str()?;
    let content = content_text(value.get("content"));
    (!content.trim().is_empty()).then(|| Message::new(role, content))
}

/// Extracts textual content from string or multipart JSON content.
#[must_use]
pub fn content_text(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    if let Some(text) = value.as_str() {
        return text.to_owned();
    }
    if let Some(parts) = value.as_array() {
        let text = parts
            .iter()
            .filter_map(part_text)
            .collect::<Vec<_>>()
            .join("\n");
        return text;
    }
    value.to_string()
}

fn part_text(value: &Value) -> Option<&str> {
    value
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| value.get("content").and_then(Value::as_str))
}
