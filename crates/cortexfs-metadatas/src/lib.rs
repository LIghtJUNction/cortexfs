#![forbid(unsafe_code)]

//! Provider-neutral model metadata and alias registry.

#[doc(hidden)]
pub mod catalog;
#[doc(hidden)]
pub mod context;
#[doc(hidden)]
pub mod error;
#[doc(hidden)]
pub mod metadata;
#[doc(hidden)]
pub mod model;
#[doc(hidden)]
pub(crate) mod remote;
#[doc(hidden)]
pub mod source;
#[doc(hidden)]
pub mod validate;
#[doc(hidden)]
pub mod validation;

pub use catalog::MetadataCatalog;
pub use context::{
    ContextWindowPolicy, DEFAULT_COMPACTION_THRESHOLD_PERCENT, DEFAULT_RECOMMENDED_CONTEXT_PERCENT,
    DEFAULT_RECOMMENDED_CONTEXT_TOKENS, compaction_threshold_tokens, recommended_context_tokens,
};
pub use error::{MetadataError, MetadataSourceError};
pub use metadata::ModelMetadata;
pub use model::{Modality, ModelStatus, ReasoningMetadata, Support};
pub use source::{MODEL_METADATA_SCHEMA, MetadataSource, SourceConfidence};
