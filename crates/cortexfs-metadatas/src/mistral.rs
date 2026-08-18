use crate::common::official;
use crate::{Modality, ModelMetadata, Support};

const MODELS: &str = "https://docs.mistral.ai/models";
const TOOLS: &str = "https://docs.mistral.ai/studio/conversations/function-calling";

pub(crate) fn models() -> Vec<ModelMetadata> {
    let vision = [Modality::Text, Modality::Image];
    let make = |id: &str, alias: &str, name: &str, context: u32| {
        official("mistral", id, name, MODELS)
            .with_aliases([alias])
            .with_context(context)
            .with_modalities(&vision, &[Modality::Text])
            .with_capabilities(Support::Supported, Support::Supported, Support::Supported)
            .with_source(crate::MetadataSource::official("Mistral AI", TOOLS))
    };
    let audio = [Modality::Text, Modality::Audio];
    vec![
        make(
            "mistral-large-2512",
            "mistral-large-latest",
            "Mistral Large 3",
            262_144,
        ),
        make(
            "mistral-medium-3-5",
            "mistral-medium-latest",
            "Mistral Medium 3.5",
            262_144,
        ),
        official("mistral", "ministral-14b-2512", "Ministral 3 14B", MODELS)
            .with_context(262_144)
            .with_modalities(&vision, &[Modality::Text])
            .with_capabilities(Support::Supported, Support::Supported, Support::Supported),
        official("mistral", "voxtral-small-2507", "Voxtral Small", MODELS)
            .with_context(32_768)
            .with_modalities(&audio, &[Modality::Text])
            .with_capabilities(Support::Supported, Support::Supported, Support::Supported),
    ]
}
