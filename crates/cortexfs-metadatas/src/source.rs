use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Date of the checked-in official catalog snapshot.
pub const CATALOG_DATE: &str = "2026-08-16";
/// Default upstream location for remote model metadata.
pub const MODELS_DEV_ENDPOINT: &str = "https://models.dev";
/// Cache file for the flattened runtime catalog payload.
pub const MODEL_METADATA_CACHE_FILE: &str = "model-metadata.json";
/// Serialized cache schema.
pub const MODEL_METADATA_SCHEMA: &str = "cortexfs.model-metadata/v1";
/// Maximum cache size in bytes.
pub const MAX_CACHE_BYTES: usize = 16 * 1024 * 1024;
/// Maximum number of cached canonical model records.
pub const MAX_CACHED_MODELS: usize = 8192;

/// Canonicalized cache payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CachedMetadataCatalog {
    pub schema: String,
    pub models: BTreeMap<String, crate::ModelMetadata>,
}

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
