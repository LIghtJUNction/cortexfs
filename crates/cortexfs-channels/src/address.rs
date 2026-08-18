use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ChannelError;

/// Canonical adapter identifier, such as `telegram` or `slack`.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelId(String);

impl ChannelId {
    /// Constructs an id from a compile-time platform name owned by the crate.
    ///
    /// Callers receiving external configuration should use [`Self::new`].
    #[must_use]
    pub fn from_static(value: &'static str) -> Self {
        Self(value.to_owned())
    }

    pub fn new(value: impl Into<String>) -> Result<Self, ChannelError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 64
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'-' | b'_')
            })
        {
            return Err(ChannelError::InvalidValue(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the platform family portion of this id.
    ///
    /// The complete value identifies one running channel instance. An
    /// optional suffix after the first dot identifies that instance, so
    /// `telegram.primary` has the family `telegram`.
    #[must_use]
    pub fn family(&self) -> &str {
        self.0
            .split_once('.')
            .map_or(self.as_str(), |(family, _)| family)
    }

    /// Returns the optional instance name carried by this id.
    #[must_use]
    pub fn instance(&self) -> Option<&str> {
        self.0
            .split_once('.')
            .and_then(|(_, instance)| (!instance.is_empty()).then_some(instance))
    }
}

impl fmt::Display for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Platform-owned conversation identifier. It may contain opaque provider IDs.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConversationId(String);

impl ConversationId {
    pub fn new(value: impl Into<String>) -> Result<Self, ChannelError> {
        let value = value.into();
        if value.is_empty() || value.len() > 256 || value.contains('\0') {
            return Err(ChannelError::InvalidValue(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ConversationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
