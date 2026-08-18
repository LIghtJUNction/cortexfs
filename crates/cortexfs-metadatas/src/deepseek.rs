use crate::common::official;
use crate::{Modality, ModelMetadata, Support};

const PRICING: &str = "https://api-docs.deepseek.com/quick_start/pricing-details-usd";
const THINKING: &str = "https://api-docs.deepseek.com/guides/thinking_mode";

pub(crate) fn models() -> Vec<ModelMetadata> {
    let text = [Modality::Text];
    let v4 = |id: &str, name: &str| {
        official(
            "deepseek",
            id,
            name,
            "https://api-docs.deepseek.com/news/news260424",
        )
        .with_context(1_000_000)
        .with_modalities(&text, &text)
        .with_capabilities(Support::Supported, Support::Supported, Support::Supported)
        .with_reasoning(&["high", "max"], "reasoning_effort")
        .with_source(crate::MetadataSource::official("DeepSeek", THINKING))
    };
    let classic = |id: &str, name: &str, reasoning: bool| {
        let mut model = official("deepseek", id, name, PRICING)
            .with_context(65_536)
            .with_max_output(8_192)
            .with_modalities(&text, &text)
            .with_capabilities(Support::Supported, Support::Unknown, Support::Supported);
        if reasoning {
            model = model.with_reasoning(&["high", "max"], "reasoning_effort");
        }
        model
    };
    vec![
        v4("deepseek-v4-flash", "DeepSeek V4 Flash"),
        v4("deepseek-v4-pro", "DeepSeek V4 Pro"),
        classic("deepseek-chat", "DeepSeek Chat", false),
        classic("deepseek-reasoner", "DeepSeek Reasoner", true),
    ]
}
