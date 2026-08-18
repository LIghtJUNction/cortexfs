#![forbid(unsafe_code)]

//! Provider-neutral model metadata and alias registry.

#[doc(hidden)]
pub mod anthropic;
#[doc(hidden)]
pub mod catalog;
#[doc(hidden)]
pub mod common;
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
pub mod source;
#[doc(hidden)]
pub mod xai;

pub use catalog::MetadataCatalog;
pub use error::MetadataError;
pub use metadata::ModelMetadata;
pub use model::{Modality, ModelStatus, ReasoningMetadata, Support};
pub use source::{CATALOG_DATE, MetadataSource, SourceConfidence};

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
