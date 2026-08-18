use crate::common::official;
use crate::{Modality, ModelMetadata, Support};

const MODELS: &str = "https://ai.google.dev/gemini-api/docs/models";

pub(crate) fn models() -> Vec<ModelMetadata> {
    let multimodal = [
        Modality::Text,
        Modality::Image,
        Modality::Audio,
        Modality::Video,
        Modality::Pdf,
    ];
    let make = |id: &str, name: &str, output: u32| {
        official(
            "google",
            id,
            name,
            "https://ai.google.dev/gemini-api/docs/models/gemini-2.5-pro",
        )
        .with_context(1_048_576)
        .with_max_output(output)
        .with_modalities(&multimodal, &[Modality::Text])
        .with_capabilities(Support::Supported, Support::Supported, Support::Supported)
        .with_reasoning(&["low", "medium", "high"], "thinkingConfig.thinkingBudget")
    };
    vec![
        make("gemini-2.5-pro", "Gemini 2.5 Pro", 65_536),
        make("gemini-2.5-flash", "Gemini 2.5 Flash", 65_536),
        official("google", "gemini-3.1-pro-preview", "Gemini 3.1 Pro", MODELS)
            .with_context(1_048_576)
            .with_max_output(65_536)
            .with_modalities(&multimodal, &[Modality::Text])
            .with_capabilities(Support::Supported, Support::Supported, Support::Supported)
            .with_reasoning(&["low", "medium", "high"], "thinkingConfig.thinkingLevel"),
    ]
}
