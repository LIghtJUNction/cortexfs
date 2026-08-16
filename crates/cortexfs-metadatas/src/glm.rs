use crate::common::official;
use crate::{Modality, ModelMetadata, Support};

const MODELS: &str = "https://docs.z.ai/guides/overview/overview";
const PARAMETERS: &str = "https://docs.z.ai/guides/overview/concept-param";

pub(crate) fn models() -> Vec<ModelMetadata> {
    let text = [Modality::Text];
    let make = |id: &str, name: &str, context: u32, output: u32| {
        official("zai", id, name, MODELS)
            .with_context(context)
            .with_max_output(output)
            .with_modalities(&text, &text)
            .with_capabilities(Support::Supported, Support::Supported, Support::Supported)
            .with_reasoning(&["disabled", "enabled"], "thinking.type")
            .with_source(crate::MetadataSource::official("Z.AI", PARAMETERS))
    };
    let vision = [Modality::Text, Modality::Image, Modality::Video];
    vec![
        make("glm-5.1", "GLM-5.1", 200_000, 131_072),
        make("glm-4.7", "GLM-4.7", 200_000, 131_072),
        official("zai", "glm-5v-turbo", "GLM-5V-Turbo", MODELS)
            .with_context(200_000)
            .with_max_output(131_072)
            .with_modalities(&vision, &text)
            .with_capabilities(Support::Supported, Support::Supported, Support::Supported)
            .with_reasoning(&["disabled", "enabled"], "thinking.type"),
        official("zai", "glm-4.6v", "GLM-4.6V", MODELS)
            .with_context(131_072)
            .with_max_output(32_768)
            .with_modalities(&vision, &text)
            .with_capabilities(Support::Supported, Support::Supported, Support::Supported)
            .with_reasoning(&["disabled", "enabled"], "thinking.type"),
    ]
}
