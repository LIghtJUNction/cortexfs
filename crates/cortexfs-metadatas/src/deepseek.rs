use crate::common::official;
use crate::{Modality, ModelMetadata, Support};
use serde_json::json;

const PRICING: &str = "https://api-docs.deepseek.com/quick_start/pricing-details-usd";
const THINKING: &str = "https://api-docs.deepseek.com/guides/thinking_mode";
const MODELS_DEV: &str = "https://models.dev/api.json";

pub(crate) fn models() -> Vec<ModelMetadata> {
    let text = [Modality::Text];
    let v4 = |id: &str, name: &str| {
        let (
            description,
            family,
            knowledge,
            release,
            open_weights,
            levels,
            input,
            output,
            reasoning,
            cache_read,
        ) = match id {
            "deepseek-v4-flash" => (
                "Official DeepSeek V4 Flash release with enhanced agentic capabilities and integrated DSpark speculative decoding",
                "deepseek-flash",
                Some("2025-05"),
                "2026-07-31",
                true,
                &["low", "high", "max"][..],
                0.14,
                0.28,
                0.28,
                0.0028,
            ),
            "deepseek-v4-pro" => (
                "DeepSeek V4 Pro snapshot with million-token context and support for thinking and non-thinking modes",
                "deepseek-thinking",
                None,
                "2026-08-12",
                false,
                &["high", "max"][..],
                0.435,
                0.87,
                0.87,
                0.003_625,
            ),
            _ => return ModelMetadata::new("deepseek", id, name),
        };
        let mut model = official(
            "deepseek",
            id,
            name,
            "https://api-docs.deepseek.com/news/news260424",
        )
        .with_context(1_000_000)
        .with_modalities(&text, &text)
        .with_capabilities(Support::Supported, Support::Supported, Support::Supported)
        .with_reasoning(levels, "reasoning_effort")
        .with_source(crate::MetadataSource::official("DeepSeek", THINKING));
        model.description = Some(description.to_owned());
        model.family = Some(family.to_owned());
        model.knowledge = knowledge.map(str::to_owned);
        model.release_date = Some(release.to_owned());
        model.last_updated = Some(release.to_owned());
        model.attachment = Support::Unsupported;
        model.temperature = Support::Supported;
        model.open_weights = if open_weights {
            Support::Supported
        } else {
            Support::Unsupported
        };
        model.interleaved = Support::Supported;
        let mut models_dev = json!({
            "id": id,
            "name": name,
            "description": description,
            "family": family,
            "attachment": false,
            "reasoning": true,
            "reasoning_options": [{"type": "toggle"}, {"type": "effort", "values": levels}],
            "tool_call": true,
            "interleaved": {"field": "reasoning_content"},
            "structured_output": true,
            "temperature": true,
            "release_date": release,
            "last_updated": release,
            "modalities": {"input": ["text"], "output": ["text"]},
            "open_weights": open_weights,
            "limit": {"context": 1_000_000, "output": 384_000},
            "cost": {"input": input, "output": output, "reasoning": reasoning, "cache_read": cache_read}
        });
        if let Some(knowledge) = knowledge
            && let Some(object) = models_dev.as_object_mut()
        {
            object.insert("knowledge".to_owned(), json!(knowledge));
        }
        model.models_dev = Some(models_dev);
        model = model.with_source(crate::MetadataSource::official("models.dev", MODELS_DEV));
        model
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
        v4("deepseek-v4-flash", "DeepSeek V4 Flash").with_aliases(["deepseek-v4-flash-0731"]),
        v4("deepseek-v4-pro", "DeepSeek V4 Pro"),
        classic("deepseek-chat", "DeepSeek Chat", false),
        classic("deepseek-reasoner", "DeepSeek Reasoner", true),
    ]
}
