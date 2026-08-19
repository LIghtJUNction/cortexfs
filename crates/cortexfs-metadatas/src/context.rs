//! Conservative context-window policy derived from a model hard limit.

use serde::{Deserialize, Serialize};

use crate::ModelMetadata;

/// Default maximum working window used when a provider advertises a larger limit.
pub const DEFAULT_RECOMMENDED_CONTEXT_TOKENS: u32 = 131_072;
/// Fraction of the recommended window at which a context compiler should compact.
pub const DEFAULT_COMPACTION_THRESHOLD_PERCENT: u32 = 80;

/// Three distinct context limits exposed to runtimes and filesystem clients.
#[expect(
    clippy::struct_field_names,
    reason = "token units are part of the public context policy contract"
)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContextWindowPolicy {
    pub max_tokens: Option<u32>,
    pub recommended_tokens: Option<u32>,
    pub compaction_threshold_tokens: Option<u32>,
}

impl ContextWindowPolicy {
    /// Creates an unknown policy for a model without a trusted hard limit.
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            max_tokens: None,
            recommended_tokens: None,
            compaction_threshold_tokens: None,
        }
    }
}

/// Derives a bounded working window from a provider/model hard maximum.
#[must_use]
pub const fn recommended_context_tokens(max_tokens: u32) -> u32 {
    if max_tokens < DEFAULT_RECOMMENDED_CONTEXT_TOKENS {
        max_tokens
    } else {
        DEFAULT_RECOMMENDED_CONTEXT_TOKENS
    }
}

/// Derives the compaction trigger from a recommended working window.
#[must_use]
pub const fn compaction_threshold_tokens(recommended_tokens: u32) -> u32 {
    let threshold = recommended_tokens.saturating_mul(DEFAULT_COMPACTION_THRESHOLD_PERCENT) / 100;
    if threshold == 0 { 1 } else { threshold }
}

impl ModelMetadata {
    /// Returns the hard, recommended, and compaction context policy.
    #[must_use]
    pub const fn context_policy(&self) -> ContextWindowPolicy {
        let Some(max_tokens) = self.context_window_tokens else {
            return ContextWindowPolicy::unknown();
        };
        if max_tokens == 0 {
            return ContextWindowPolicy::unknown();
        }
        let recommended_tokens = match self.recommended_context_tokens {
            Some(tokens) => {
                let bounded = if tokens < max_tokens {
                    tokens
                } else {
                    max_tokens
                };
                if bounded == 0 { 1 } else { bounded }
            }
            None => recommended_context_tokens(max_tokens),
        };
        let compaction_threshold_tokens = match self.compaction_threshold_tokens {
            Some(tokens) => {
                let bounded = if tokens < recommended_tokens {
                    tokens
                } else {
                    recommended_tokens
                };
                if bounded == 0 { 1 } else { bounded }
            }
            None => compaction_threshold_tokens(recommended_tokens),
        };
        ContextWindowPolicy {
            max_tokens: Some(max_tokens),
            recommended_tokens: Some(recommended_tokens),
            compaction_threshold_tokens: Some(compaction_threshold_tokens),
        }
    }
}
