use serde::{Deserialize, Serialize};

/// Identifies an opaque context handle owned by an upstream provider.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContextReference {
    pub namespace: String,
    pub value: String,
}

/// Identifies who is responsible for retaining conversation history.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextOwnership {
    #[default]
    ClientOwned,
    ProviderOwned,
    Hybrid,
}

/// Describes how an adapter should replay or materialize context.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayPolicy {
    #[default]
    FullHistory,
    MaterializeHistory,
    ReferenceOnly,
}

/// Context semantics carried with the provider-neutral request IR.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContextState {
    pub ownership: ContextOwnership,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<ContextReference>,
    pub replay: ReplayPolicy,
}

impl ContextState {
    /// Creates the portable mode in which the client supplies the history.
    #[must_use]
    pub fn client_owned() -> Self {
        Self::default()
    }

    /// Creates a provider-owned mode with an opaque provider reference.
    #[must_use]
    pub fn provider_owned(namespace: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            ownership: ContextOwnership::ProviderOwned,
            reference: Some(ContextReference {
                namespace: namespace.into(),
                value: value.into(),
            }),
            replay: ReplayPolicy::ReferenceOnly,
        }
    }

    /// Returns whether a route must materialize history before conversion.
    #[must_use]
    pub fn requires_materialization(&self) -> bool {
        self.replay == ReplayPolicy::MaterializeHistory
            || (self.ownership == ContextOwnership::ProviderOwned && self.reference.is_none())
    }

    /// Checks that opaque references are present when the selected policy needs one.
    pub fn validate(&self) -> Result<(), crate::ProtocolError> {
        if matches!(self.ownership, ContextOwnership::ProviderOwned) && self.reference.is_none() {
            return Err(crate::ProtocolError::InvalidContext(
                "provider-owned context requires a reference".to_owned(),
            ));
        }
        if self.replay == ReplayPolicy::ReferenceOnly && self.reference.is_none() {
            return Err(crate::ProtocolError::InvalidContext(
                "reference-only replay requires a reference".to_owned(),
            ));
        }
        Ok(())
    }
}
