use serde::{Deserialize, Serialize};

/// Normalized credential kind used by provider adapters.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    /// Static API key material.
    ApiKey,
    /// OAuth bearer material and optional refresh metadata.
    OAuth,
}

/// In-memory credential envelope shared by authentication adapters.
///
/// The envelope is not a `/ctx` object and must only be persisted through the
/// existing root-owned secret store or an equivalent OS credential backend.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Credential {
    /// API-key credential.
    ApiKey {
        provider: String,
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        slot: Option<String>,
    },
    /// OAuth credential with optional refresh and expiry metadata.
    OAuth {
        provider: String,
        access_token: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refresh_token: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires_at: Option<u64>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        scopes: Vec<String>,
    },
}

impl Credential {
    /// Returns the stable provider identity carried by this credential.
    #[must_use]
    pub fn provider(&self) -> &str {
        match self {
            &Self::ApiKey { ref provider, .. } | &Self::OAuth { ref provider, .. } => provider,
        }
    }

    /// Returns the normalized credential kind.
    #[must_use]
    pub const fn kind(&self) -> CredentialKind {
        match *self {
            Self::ApiKey { .. } => CredentialKind::ApiKey,
            Self::OAuth { .. } => CredentialKind::OAuth,
        }
    }

    /// Returns whether the optional OAuth expiry is at or before `now`.
    #[must_use]
    pub fn is_expired(&self, now: u64) -> bool {
        matches!(self, Self::OAuth { expires_at: Some(expires_at), .. } if *expires_at <= now)
    }
}
