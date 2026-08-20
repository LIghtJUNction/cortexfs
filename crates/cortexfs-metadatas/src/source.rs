use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Default upstream location for remote model metadata.
pub const MODELS_DEV_ENDPOINT: &str = "https://models.dev";
/// Cache file for the flattened runtime catalog payload.
pub const MODEL_METADATA_CACHE_FILE: &str = "model-metadata.json";
/// Serialized cache schema.
pub const MODEL_METADATA_SCHEMA: &str = "cortexfs.model-metadata/v2";
/// Maximum cache size in bytes.
pub const MAX_CACHE_BYTES: usize = 16 * 1024 * 1024;
/// Maximum number of cached canonical model records.
pub const MAX_CACHED_MODELS: usize = 8192;

/// Canonicalized cache payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CachedMetadataCatalog {
    pub schema: String,
    #[serde(default)]
    pub observed_on: String,
    pub catalog: Value,
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
    /// Creates an official source; refreshes fill `observed_on` from HTTP.
    #[must_use]
    pub fn official(publisher: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            publisher: publisher.into(),
            url: url.into(),
            observed_on: String::new(),
            confidence: SourceConfidence::Official,
        }
    }
}
