use crate::is_object_name;

/// Provider-neutral behavior loop selected for an executable agent.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum AgentLoop {
    /// One input produces one response.
    #[default]
    Chat,
    /// The agent may alternate model actions and observations.
    React,
    /// The agent follows a coding-oriented action loop.
    Coding,
    /// The agent separates planning from execution.
    Planner,
    /// The agent performs bounded research steps.
    Research,
    /// A runtime-known custom loop name.
    Custom(String),
}

impl AgentLoop {
    /// Parses the optional `agent/<name>.d/loop` control value.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "chat" => Some(Self::Chat),
            "react" => Some(Self::React),
            "coding" => Some(Self::Coding),
            "planner" => Some(Self::Planner),
            "research" => Some(Self::Research),
            value if is_object_name(value) => Some(Self::Custom(value.to_owned())),
            _ => None,
        }
    }

    /// Returns the stable control and environment value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match *self {
            Self::Chat => "chat",
            Self::React => "react",
            Self::Coding => "coding",
            Self::Planner => "planner",
            Self::Research => "research",
            Self::Custom(ref value) => value,
        }
    }
}
