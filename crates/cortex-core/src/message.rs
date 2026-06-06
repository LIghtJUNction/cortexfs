use crate::ValidationError;
use core::fmt;
use core::str::FromStr;

/// Chat message role used by the training-friendly `messages.jsonl` ABI.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

impl MessageRole {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

impl fmt::Display for MessageRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MessageRole {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "system" => Ok(Self::System),
            "user" => Ok(Self::User),
            "assistant" => Ok(Self::Assistant),
            "tool" => Ok(Self::Tool),
            _ => Err(ValidationError::unsupported_message_role(value)),
        }
    }
}

/// Minimal message object for the `messages.jsonl` ABI.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Message {
    role: MessageRole,
    content: String,
}

impl Message {
    #[must_use]
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }

    #[must_use]
    pub const fn role(&self) -> MessageRole {
        self.role
    }

    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }

    #[must_use]
    pub fn into_content(self) -> String {
        self.content
    }
}

#[cfg(test)]
mod tests {
    use super::{Message, MessageRole};
    use core::str::FromStr;

    #[test]
    fn message_role_round_trips_stable_names() {
        assert_eq!(MessageRole::Assistant.as_str(), "assistant");
        assert_eq!(MessageRole::from_str("tool"), Ok(MessageRole::Tool));
        assert!(MessageRole::from_str("developer").is_err());
    }

    #[test]
    fn message_keeps_role_and_content() {
        let message = Message::new(MessageRole::User, "hello");

        assert_eq!(message.role(), MessageRole::User);
        assert_eq!(message.content(), "hello");
    }
}
