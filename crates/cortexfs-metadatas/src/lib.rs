#![forbid(unsafe_code)]

//! Provider-neutral model metadata and alias registry.

#[doc(hidden)]
pub mod anthropic;
#[doc(hidden)]
pub mod catalog;
#[doc(hidden)]
pub mod common;
#[doc(hidden)]
pub mod context;
#[doc(hidden)]
pub mod deepseek;
#[doc(hidden)]
pub mod error;
#[doc(hidden)]
pub mod glm;
#[doc(hidden)]
pub mod google;
#[doc(hidden)]
pub mod lookup;
#[doc(hidden)]
pub mod metadata;
#[doc(hidden)]
pub mod mistral;
#[doc(hidden)]
pub mod model;
#[doc(hidden)]
pub mod openai;
#[doc(hidden)]
pub mod qwen;
#[doc(hidden)]
pub(crate) mod remote;
#[doc(hidden)]
pub mod source;
#[doc(hidden)]
pub mod validate;
#[doc(hidden)]
pub mod validation;
#[doc(hidden)]
pub mod xai;

pub use catalog::MetadataCatalog;
pub use context::{
    ContextWindowPolicy, DEFAULT_COMPACTION_THRESHOLD_PERCENT, DEFAULT_RECOMMENDED_CONTEXT_TOKENS,
    compaction_threshold_tokens, recommended_context_tokens,
};
pub use error::{MetadataError, MetadataSourceError};
pub use metadata::ModelMetadata;
pub use model::{Modality, ModelStatus, ReasoningMetadata, Support};
pub use source::{CATALOG_DATE, MODEL_METADATA_SCHEMA, MetadataSource, SourceConfidence};

pub(crate) fn builtin_models() -> Vec<ModelMetadata> {
    [
        anthropic::models(),
        deepseek::models(),
        google::models(),
        glm::models(),
        mistral::models(),
        openai::models(),
        qwen::models(),
        xai::models(),
    ]
    .into_iter()
    .flatten()
    .collect()
}
