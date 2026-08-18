use crate::common::official;
use crate::{Modality, ModelMetadata, Support};

const API: &str = "https://help.aliyun.com/zh/model-studio/model-qwen3-max";
const TOOLS: &str =
    "https://github.com/QwenLM/Qwen3/blob/main/docs/source/getting_started/concepts.md";

pub(crate) fn models() -> Vec<ModelMetadata> {
    let text = [Modality::Text];
    let thinking = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];
    let max = official("qwen", "qwen3-max", "Qwen3 Max", API)
        .with_aliases(["qwen3-max-2026-01-23"])
        .with_context(262_144)
        .with_max_output(65_536)
        .with_modalities(&text, &text)
        .with_capabilities(Support::Supported, Support::Supported, Support::Supported)
        .with_reasoning(&thinking, "reasoning.effort")
        .with_source(crate::MetadataSource::official("Qwen", TOOLS));
    let coder = official(
        "qwen",
        "qwen3-coder-480b-a35b-instruct",
        "Qwen3 Coder 480B",
        TOOLS,
    )
    .with_context(262_144)
    .with_modalities(&text, &text)
    .with_capabilities(Support::Supported, Support::Supported, Support::Supported);
    let thinking_model = official(
        "qwen",
        "qwen3-235b-a22b-thinking-2507",
        "Qwen3 235B A22B Thinking 2507",
        TOOLS,
    )
    .with_context(262_144)
    .with_max_output(81_920)
    .with_modalities(&text, &text)
    .with_capabilities(Support::Supported, Support::Supported, Support::Supported)
    .with_reasoning(&["enabled"], "enable_thinking");
    vec![max, coder, thinking_model]
}
