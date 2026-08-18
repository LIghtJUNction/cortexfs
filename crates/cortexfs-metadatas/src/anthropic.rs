use crate::common::official;
use crate::{Modality, ModelMetadata, Support};

const MODELS: &str = "https://platform.claude.com/docs/en/about-claude/models/overview";

pub(crate) fn models() -> Vec<ModelMetadata> {
    let multimodal = [Modality::Text, Modality::Image];
    let make = |id: &str, name: &str, context: u32, output: u32| {
        official("anthropic", id, name, MODELS)
            .with_context(context)
            .with_max_output(output)
            .with_modalities(&multimodal, &[Modality::Text])
            .with_capabilities(Support::Supported, Support::Supported, Support::Supported)
            .with_reasoning(&["none", "low", "medium", "high"], "thinking.type")
    };
    vec![
        make("claude-opus-4-7", "Claude Opus 4.7", 1_000_000, 128_000),
        make("claude-sonnet-4-6", "Claude Sonnet 4.6", 1_000_000, 64_000),
        make(
            "claude-haiku-4-5-20251001",
            "Claude Haiku 4.5",
            200_000,
            64_000,
        )
        .with_aliases(["claude-haiku-4-5"]),
    ]
}
