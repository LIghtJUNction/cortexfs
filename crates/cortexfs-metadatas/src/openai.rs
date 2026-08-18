use crate::common::official;
use crate::{Modality, ModelMetadata, Support};

const MODELS: &str = "https://developers.openai.com/api/docs/models";

pub(crate) fn models() -> Vec<ModelMetadata> {
    let reasoning = ["none", "low", "medium", "high", "xhigh", "max"];
    let frontier = |id: &str, name: &str| {
        official("openai", id, name, MODELS)
            .with_context(1_050_000)
            .with_max_output(128_000)
            .with_modalities(&[Modality::Text, Modality::Image], &[Modality::Text])
            .with_capabilities(Support::Supported, Support::Supported, Support::Supported)
            .with_reasoning(&reasoning, "reasoning.effort")
    };
    let mut gpt41 = official(
        "openai",
        "gpt-4.1",
        "GPT-4.1",
        "https://developers.openai.com/api/docs/models/gpt-4.1",
    )
    .with_context(1_047_576)
    .with_max_output(32_768)
    .with_modalities(&[Modality::Text, Modality::Image], &[Modality::Text])
    .with_capabilities(Support::Supported, Support::Supported, Support::Supported);
    gpt41.reasoning.support = Support::Unsupported;
    vec![
        frontier("gpt-5.6-sol", "GPT-5.6 Sol").with_aliases(["gpt-5.6"]),
        frontier("gpt-5.6-terra", "GPT-5.6 Terra"),
        frontier("gpt-5.6-luna", "GPT-5.6 Luna"),
        official(
            "openai",
            "gpt-5.2",
            "GPT-5.2",
            "https://developers.openai.com/api/docs/models/gpt-5.2",
        )
        .with_context(400_000)
        .with_max_output(128_000)
        .with_modalities(&[Modality::Text, Modality::Image], &[Modality::Text])
        .with_capabilities(Support::Supported, Support::Supported, Support::Supported)
        .with_reasoning(
            &["none", "low", "medium", "high", "xhigh"],
            "reasoning.effort",
        ),
        gpt41,
    ]
}
