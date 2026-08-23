use crate::is_object_name;

/// How a tool executable is invoked by the host.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum InvokeStrategy {
    /// Use the default tool executable; caller selects CLI or SDK mode.
    #[default]
    Default,
    /// Force terminal CLI argv/stdin semantics.
    Cli,
    /// Force structured Tool SDK JSONL semantics.
    Sdk,
    /// Run `tool/<name>.d/invoke.d/<name>` instead of the default executable.
    Custom(String),
}

impl InvokeStrategy {
    /// Parses `tool/<name>.d/invoke.strategy`.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "default" => Some(Self::Default),
            "cli" => Some(Self::Cli),
            "sdk" => Some(Self::Sdk),
            name if is_object_name(name) => Some(Self::Custom(name.to_owned())),
            _ => None,
        }
    }

    /// Returns the stable control value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            &Self::Default => "default",
            &Self::Cli => "cli",
            &Self::Sdk => "sdk",
            &Self::Custom(ref value) => value,
        }
    }
}
