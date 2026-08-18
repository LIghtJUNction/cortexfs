use crate::common::official;
use crate::{Modality, ModelMetadata, Support};

const MODELS: &str = "https://docs.x.ai/developers/models";

pub(crate) fn models() -> Vec<ModelMetadata> {
    let multimodal = [Modality::Text, Modality::Image];
    let make = |id: &str, name: &str| {
        official("xai", id, name, MODELS)
            .with_context(1_000_000)
            .with_modalities(&multimodal, &[Modality::Text])
            .with_capabilities(Support::Supported, Support::Supported, Support::Supported)
            .with_reasoning(&["low", "medium", "high"], "reasoning_effort")
    };
    vec![
        make("grok-4.3", "Grok 4.3").with_aliases(["grok-latest"]),
        make("grok-4.20-multi-agent-0309", "Grok 4.20 Multi-Agent").with_aliases([
            "grok-4.20-multi-agent",
            "grok-4.20-multi-agent-latest",
            "grok-4.20-multi-agent-beta-latest",
        ]),
    ]
}
