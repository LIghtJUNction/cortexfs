use crate::{MetadataSource, ModelMetadata};

pub(crate) fn official(provider: &str, id: &str, name: &str, url: &str) -> ModelMetadata {
    ModelMetadata::new(provider, id, name).with_source(MetadataSource::official(provider, url))
}
