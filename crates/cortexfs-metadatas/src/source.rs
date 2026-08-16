use serde::{Deserialize, Serialize};

/// Date of the checked-in official catalog snapshot.
pub const CATALOG_DATE: &str = "2026-08-16";

/// Provenance confidence for a metadata record.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceConfidence {
    Official,
    Community,
    Inferred,
}

/// A source supporting one or more fields of a model record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MetadataSource {
    pub publisher: String,
    pub url: String,
    pub observed_on: String,
    pub confidence: SourceConfidence,
}

impl MetadataSource {
    /// Creates an official source using the catalog snapshot date.
    #[must_use]
    pub fn official(publisher: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            publisher: publisher.into(),
            url: url.into(),
            observed_on: CATALOG_DATE.to_owned(),
            confidence: SourceConfidence::Official,
        }
    }
}
