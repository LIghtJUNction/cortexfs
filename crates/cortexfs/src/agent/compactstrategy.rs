use crate::is_object_name;

/// How durable history is rebuilt into prompt context.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum CompactStrategy {
    /// Keep the newest messages and drop older ones.
    #[default]
    Truncate,
    /// Summarize omitted messages with the built-in provider-neutral summarizer.
    Summarize,
    /// Run `agent/<name>.d/compact.d/<name>` as an external summarizer.
    Custom(String),
}

impl CompactStrategy {
    /// Parses `agent/<name>.d/compact.strategy`.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "truncate" => Some(Self::Truncate),
            "summarize" => Some(Self::Summarize),
            name if is_object_name(name) => Some(Self::Custom(name.to_owned())),
            _ => None,
        }
    }

    /// Returns the stable control value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            &Self::Truncate => "truncate",
            &Self::Summarize => "summarize",
            &Self::Custom(ref value) => value,
        }
    }
}
