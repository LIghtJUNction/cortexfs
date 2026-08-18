use std::fmt::Write as _;

use sha2::{Digest, Sha256};

/// Inputs represented by one non-secret runtime observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeObservation<'a> {
    pub run: &'a str,
    pub step: u8,
    pub action: &'a str,
    pub tool: Option<&'a str>,
    pub context_revision: Option<&'a str>,
    pub updated_at: &'a str,
}

/// Hashes compiled context inputs without returning their contents.
#[must_use]
pub fn context_revision(history: &str, tools: &str, observation: &str) -> String {
    let mut digest = Sha256::new();
    for value in [history, tools, observation] {
        digest.update(value.len().to_le_bytes());
        digest.update(value.as_bytes());
    }
    let mut hex = String::with_capacity(64);
    for byte in digest.finalize() {
        if write!(&mut hex, "{byte:02x}").is_err() {
            return "sha256:".to_owned();
        }
    }
    format!("sha256:{hex}")
}
