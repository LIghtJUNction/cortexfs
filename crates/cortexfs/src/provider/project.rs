use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;

use cortexfs_metadatas::{
    MODEL_METADATA_SCHEMA, MetadataCatalog, Modality, ModelMetadata, Support,
    compaction_threshold_tokens, recommended_context_tokens,
};

use super::catalog::cached_model_limits;
use super::config::{ProjectedProviderModel, ProviderConfig};
use super::discovery::provider_cached_models;
use crate::{ModelContextLimit, STABLE_MODEL_CAPABILITIES, is_object_name};

struct ResolvedModelMetadata<'a> {
    metadata: Option<&'a ModelMetadata>,
    context_window_tokens: Option<u32>,
    context_metadata: Option<&'a ModelMetadata>,
    log: String,
}

pub(super) fn project_models(
    provider: &str,
    config: &ProviderConfig,
    cache_dir: &Path,
    projected: &mut Vec<ProjectedProviderModel>,
) {
    let limits = cached_model_limits(cache_dir);
    let catalog = MetadataCatalog::from_cache_or_empty(cache_dir);
    let driver = driver_text(&config.formats);
    for model in model_names(config, cache_dir, provider) {
        if projected
            .iter()
            .any(|known| known.provider == provider && known.model == model)
        {
            continue;
        }

        let has_custom_limit = config.model_limits.contains_key(&model);
        let has_custom_cap = config.model_capabilities.contains_key(&model);
        let custom_metadata = has_custom_limit || has_custom_cap;
        let resolved = resolve_model_metadata(&catalog, provider, &model, custom_metadata);

        let mut cap = capability_text(
            &config.formats,
            config.model_capabilities.get(&model).map(Vec::as_slice),
            resolved.metadata,
        );
        if cap.is_empty()
            && resolved.metadata.is_none()
            && !config.model_capabilities.contains_key(&model)
        {
            cap = String::from("chat\nstream\n");
        }

        let limit = config
            .model_limits
            .get(&model)
            .copied()
            .or(resolved.context_window_tokens)
            .or_else(|| limits.get(&format!("{provider}/{model}")).copied())
            .and_then(ModelContextLimit::known)
            .unwrap_or(ModelContextLimit::Unknown);
        let (recommended, compact) = context_controls(resolved.context_metadata, limit);
        let metadata = model_metadata_document(
            provider,
            &model,
            resolved.metadata,
            limit,
            recommended,
            compact,
        );

        projected.push(ProjectedProviderModel {
            provider: provider.to_owned(),
            model,
            base_url: config.base_url.trim().to_owned(),
            driver: driver.clone(),
            log: resolved.log,
            cap,
            effort: "auto".to_owned(),
            limit,
            recommended,
            compact,
            metadata,
        });
    }
}

fn resolve_model_metadata<'a>(
    catalog: &'a MetadataCatalog,
    provider: &str,
    model: &str,
    has_custom_metadata: bool,
) -> ResolvedModelMetadata<'a> {
    let provider_key = format!("{provider}/{model}");
    if let Some(metadata) = catalog.resolve(model)
        && catalog.canonical_key(model) != Some(provider_key.as_str())
    {
        return ResolvedModelMetadata {
            metadata: Some(metadata),
            context_window_tokens: metadata.context_window_tokens,
            context_metadata: Some(metadata),
            log: String::new(),
        };
    }
    if let Some(metadata) = catalog.resolve_for(provider, model) {
        return ResolvedModelMetadata {
            metadata: Some(metadata),
            context_window_tokens: metadata.context_window_tokens,
            context_metadata: Some(metadata),
            log: String::new(),
        };
    }

    let mut warning = String::new();
    if has_custom_metadata {
        let _ignored = writeln!(
            warning,
            "WARN: 未识别基础模型 {provider}/{model}；已发现对该模型的自定义 model_limits/model_capabilities 配置，并已应用；本地配置之外的元数据保持 unknown。"
        );
    } else {
        let _ignored = writeln!(
            warning,
            "WARN: 未识别基础模型 {provider}/{model}；请在 provider 配置中显式定义 model_limits/model_capabilities，或提供可验证的 models.dev 映射；本地配置之外的元数据保持 unknown。"
        );
    }
    ResolvedModelMetadata {
        metadata: None,
        context_window_tokens: None,
        context_metadata: None,
        log: warning,
    }
}

fn context_controls(
    metadata: Option<&ModelMetadata>,
    limit: ModelContextLimit,
) -> (ModelContextLimit, ModelContextLimit) {
    let maximum = limit.tokens();
    let recommended = metadata
        .and_then(|model| model.context_policy().recommended_tokens)
        .or_else(|| maximum.map(recommended_context_tokens))
        .map(|value| maximum.map_or(value, |maximum| value.min(maximum)));
    let metadata_compact = metadata
        .and_then(|model| model.context_policy().compaction_threshold_tokens)
        .filter(|value| recommended.is_none_or(|window| *value <= window));
    let compact = metadata_compact
        .or_else(|| recommended.map(compaction_threshold_tokens))
        .map(|value| recommended.map_or(value, |recommended| value.min(recommended)))
        .filter(|value| *value > 0);
    (
        recommended
            .and_then(ModelContextLimit::known)
            .unwrap_or(ModelContextLimit::Unknown),
        compact
            .and_then(ModelContextLimit::known)
            .unwrap_or(ModelContextLimit::Unknown),
    )
}

fn model_names(config: &ProviderConfig, cache: &Path, provider: &str) -> Vec<String> {
    let cached = provider_cached_models(cache, provider);
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for model in config
        .default_model
        .iter()
        .chain(&config.models)
        .map(String::as_str)
        .chain(cached.iter().map(String::as_str))
    {
        let model = model.trim();
        if is_object_name(model) && seen.insert(model.to_owned()) {
            names.push(model.to_owned());
        }
    }
    names
}

fn driver_text(formats: &[String]) -> String {
    let responses = formats
        .iter()
        .any(|value| value.trim() == "openai.responses");
    let chat = formats.iter().any(|value| value.trim() == "openai.chat") || !responses;
    let default = if chat {
        "openai-chat"
    } else {
        "openai-responses"
    };
    let agent = if responses && chat {
        "openai-responses,openai-chat"
    } else {
        default
    };
    format!("default={default}\nexec={default}\nagent={agent}\n")
}

fn capability_text(
    formats: &[String],
    configured: Option<&[String]>,
    metadata: Option<&ModelMetadata>,
) -> String {
    if let Some(configured) = configured {
        return STABLE_MODEL_CAPABILITIES
            .iter()
            .filter(|capability| configured.iter().any(|value| value == **capability))
            .fold(String::new(), |mut output, capability| {
                let _ignored = writeln!(output, "{capability}");
                output
            });
    }

    let Some(metadata) = metadata else {
        return if formats
            .iter()
            .any(|value| value.trim() == "openai.responses")
        {
            "chat\nstream\ntool_call_syntax\n".to_owned()
        } else {
            "chat\nstream\n".to_owned()
        };
    };

    let mut cap = String::new();
    if has_text(&metadata.input_modalities, &metadata.output_modalities) {
        let _ignored = writeln!(cap, "chat");
    }
    if matches!(metadata.tools, Support::Supported) {
        let _ignored = writeln!(cap, "tool_call_syntax");
    }
    if metadata.streaming == Support::Supported
        || (metadata.streaming != Support::Unsupported && supports_streaming_format(formats))
    {
        let _ignored = writeln!(cap, "stream");
    }
    if matches!(metadata.structured_output, Support::Supported) {
        let _ignored = writeln!(cap, "json_schema");
    }
    if metadata.reasoning.support == Support::Supported {
        let _ignored = writeln!(cap, "reasoning");
    }
    if metadata.attachment == Support::Supported {
        let _ignored = writeln!(cap, "attachment");
    }
    if metadata.temperature == Support::Supported {
        let _ignored = writeln!(cap, "temperature");
    }
    if metadata.interleaved == Support::Supported {
        let _ignored = writeln!(cap, "interleaved");
    }
    if has_modalities(&metadata.input_modalities, Modality::Image)
        || has_modalities(&metadata.output_modalities, Modality::Image)
    {
        let _ignored = writeln!(cap, "vision");
    }
    if has_modalities(&metadata.input_modalities, Modality::Image) {
        let _ignored = writeln!(cap, "image_input");
    }
    if has_modalities(&metadata.output_modalities, Modality::Image) {
        let _ignored = writeln!(cap, "image_output");
    }
    if has_modalities(&metadata.input_modalities, Modality::Audio) {
        let _ignored = writeln!(cap, "audio_input");
    }
    if has_modalities(&metadata.output_modalities, Modality::Audio) {
        let _ignored = writeln!(cap, "audio_output");
    }
    if has_modalities(&metadata.input_modalities, Modality::Video) {
        let _ignored = writeln!(cap, "video_input");
    }
    if has_modalities(&metadata.output_modalities, Modality::Video) {
        let _ignored = writeln!(cap, "video_output");
    }
    if has_modalities(&metadata.input_modalities, Modality::Pdf) {
        let _ignored = writeln!(cap, "pdf_input");
    }
    if has_modalities(&metadata.output_modalities, Modality::Pdf) {
        let _ignored = writeln!(cap, "pdf_output");
    }
    if has_modalities(&metadata.input_modalities, Modality::Embedding)
        || has_modalities(&metadata.output_modalities, Modality::Embedding)
    {
        let _ignored = writeln!(cap, "embedding");
    }
    cap
}

fn supports_streaming_format(formats: &[String]) -> bool {
    formats.iter().any(|value| {
        matches!(
            value.trim(),
            "openai.chat" | "openai.responses" | "anthropic.messages" | "gemini.generate_content"
        )
    })
}

fn has_modalities(modalities: &[Modality], needle: Modality) -> bool {
    modalities.contains(&needle)
}

fn has_text(input: &[Modality], output: &[Modality]) -> bool {
    has_modalities(input, Modality::Text) || has_modalities(output, Modality::Text)
}

fn model_metadata_document(
    provider: &str,
    model: &str,
    metadata: Option<&ModelMetadata>,
    limit: ModelContextLimit,
    recommended: ModelContextLimit,
    compact: ModelContextLimit,
) -> String {
    let resolved_metadata = metadata.cloned();
    let exact = metadata.filter(|value| {
        value.provider == provider
            && (value.id == model || value.aliases.iter().any(|alias| alias == model))
    });
    let mut normalized = metadata
        .cloned()
        .unwrap_or_else(|| ModelMetadata::new(provider, model, model));
    let canonical_id = resolved_metadata.as_ref().map_or_else(
        || format!("{provider}/{model}"),
        |value| format!("{}/{}", value.provider, value.id),
    );
    let resolution = if exact.is_some() {
        "exact"
    } else if resolved_metadata.is_some() {
        "mapped"
    } else {
        "unverified"
    };
    provider.clone_into(&mut normalized.provider);
    model.clone_into(&mut normalized.id);
    serde_json::json!({
        "schema": MODEL_METADATA_SCHEMA,
        "metadata": normalized,
        "resolved_metadata": resolved_metadata,
        "canonical_id": canonical_id,
        "resolution": resolution,
        "effective": {
            "limit_tokens": limit.tokens(),
            "recommended_tokens": recommended.tokens(),
            "compaction_threshold_tokens": compact.tokens(),
        }
    })
    .to_string()
        + "\n"
}

pub fn projected_control_content(model: &ProjectedProviderModel, file: &str) -> Option<String> {
    match file {
        "id" => Some(format!("{}/{}\n", model.provider, model.model)),
        "driver" => Some(model.driver.clone()),
        "cap" => Some(model.cap.clone()),
        "effort" => Some(format!("{}\n", model.effort)),
        "limit" => Some(format!("{}\n", model.limit)),
        "recommended" => Some(format!("{}\n", model.recommended)),
        "compact" => Some(format!("{}\n", model.compact)),
        "metadata.json" => Some(model.metadata.clone()),
        "default" => Some(format!("base_url={}\n", model.base_url)),
        "session" => Some("none\n".to_owned()),
        "status" => Some("configured\n".to_owned()),
        "log" => Some(format!("{}\n", model.log.trim_end())),
        _ => None,
    }
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    clippy::similar_names,
    reason = "projection tests inspect the single validated model document directly"
)]
mod tests {
    use std::collections::HashMap;
    use std::io;
    use std::path::Path;

    use tempfile::tempdir;

    use super::*;

    fn write_local_metadata_cache(cache_dir: &Path, context: u32) -> io::Result<()> {
        write_local_raw_cache(
            cache_dir,
            &serde_json::json!({
                "id": "known", "name": "Local Known", "attachment": false,
                "reasoning": false, "tool_call": true, "open_weights": false,
                "modalities": {"input": ["text"], "output": ["text"]},
                "limit": {"context": context, "output": 0}
            }),
        )
    }

    fn write_local_raw_cache(cache_dir: &Path, model: &serde_json::Value) -> io::Result<()> {
        let content = serde_json::to_string(&serde_json::json!({
            "schema": MODEL_METADATA_SCHEMA,
            "catalog": {
                "providers": {"local": {
                    "id": "local", "name": "Local", "doc": "https://example.invalid",
                    "models": {"known": model}
                }},
                "models": {}
            }
        }))
        .map_err(|_error| io::Error::other("serialize metadata cache"))?;
        std::fs::write(cache_dir.join("model-metadata.json"), content)?;
        Ok(())
    }

    #[test]
    fn project_models_keeps_unrecognized_metadata_unverified() -> io::Result<()> {
        let dir = tempdir()?;
        write_local_metadata_cache(dir.path(), 8192)?;
        let config = ProviderConfig {
            name: None,
            base_url: "http://127.0.0.1/v1".to_owned(),
            default_model: None,
            models: vec!["mystery-model".to_owned()],
            model_limits: HashMap::new(),
            model_capabilities: HashMap::new(),
            enabled: true,
            formats: vec!["openai.chat".to_owned()],
            auth: Vec::new(),
            oauth: None,
        };

        let mut projected = Vec::new();
        project_models("local", &config, dir.path(), &mut projected);
        let model = &projected[0];

        assert_eq!(model.model, "mystery-model");
        assert_eq!(model.limit.tokens(), None);
        assert_eq!(model.recommended.tokens(), None);
        assert_eq!(model.compact.tokens(), None);
        assert!(model.cap.contains("chat"));
        assert!(
            model
                .log
                .contains("WARN: 未识别基础模型 local/mystery-model")
        );
        assert!(model.log.contains("元数据保持 unknown"));
        Ok(())
    }

    #[test]
    fn project_models_applies_local_custom_metadata_first() -> io::Result<()> {
        let dir = tempdir()?;
        write_local_metadata_cache(dir.path(), 8192)?;
        let mut model_limits = HashMap::new();
        model_limits.insert("mystery-model".to_owned(), 2048);
        let mut model_capabilities = HashMap::new();
        model_capabilities.insert(
            "mystery-model".to_owned(),
            vec!["vision".to_owned(), "session".to_owned()],
        );
        let config = ProviderConfig {
            name: None,
            base_url: "http://127.0.0.1/v1".to_owned(),
            default_model: None,
            models: vec!["mystery-model".to_owned()],
            model_limits,
            model_capabilities,
            enabled: true,
            formats: vec!["openai.chat".to_owned(), "openai.responses".to_owned()],
            auth: Vec::new(),
            oauth: None,
        };

        let mut projected = Vec::new();
        project_models("local", &config, dir.path(), &mut projected);
        let model = &projected[0];

        assert_eq!(model.model, "mystery-model");
        assert_eq!(model.limit.tokens(), Some(2048));
        assert_eq!(model.recommended.tokens(), Some(1024));
        assert_eq!(model.compact.tokens(), Some(921));
        assert_eq!(model.cap, "session\nvision\n");
        assert!(
            model
                .log
                .contains("已发现对该模型的自定义 model_limits/model_capabilities 配置，并已应用")
        );
        Ok(())
    }

    #[test]
    fn project_models_preserves_known_and_explicit_empty_capabilities() -> io::Result<()> {
        let dir = tempdir()?;
        write_local_metadata_cache(dir.path(), 8192)?;
        let mut capabilities = HashMap::new();
        capabilities.insert("unknown".to_owned(), Vec::new());
        let config = ProviderConfig {
            name: None,
            base_url: "http://127.0.0.1/v1".to_owned(),
            default_model: None,
            models: vec!["known".to_owned(), "unknown".to_owned()],
            model_limits: HashMap::new(),
            model_capabilities: capabilities,
            enabled: true,
            formats: vec!["openai.chat".to_owned()],
            auth: Vec::new(),
            oauth: None,
        };

        let mut projected = Vec::new();
        project_models("local", &config, dir.path(), &mut projected);
        assert_eq!(projected[0].cap, "chat\ntool_call_syntax\nstream\n");
        assert_eq!(projected[1].cap, "");
        Ok(())
    }

    #[test]
    fn project_models_exposes_the_complete_metadata_document() -> io::Result<()> {
        let dir = tempdir()?;
        write_local_raw_cache(
            dir.path(),
            &serde_json::json!({
                "id": "known", "name": "Local Known", "description": "official description",
                "attachment": true, "reasoning": true, "tool_call": true,
                "open_weights": false, "structured_output": true,
                "modalities": {"input": ["text"], "output": ["text"]},
                "reasoning_options": [{"type": "effort", "values": ["low", "max"]}],
                "limit": {"context": 1_000_000, "output": 0}, "future_field": "retained"
            }),
        )?;
        let config = ProviderConfig {
            name: None,
            base_url: "http://127.0.0.1/v1".to_owned(),
            default_model: None,
            models: vec!["known".to_owned()],
            model_limits: HashMap::new(),
            model_capabilities: HashMap::new(),
            enabled: true,
            formats: vec!["openai.chat".to_owned()],
            auth: Vec::new(),
            oauth: None,
        };
        let mut projected = Vec::new();
        project_models("local", &config, dir.path(), &mut projected);
        let content = projected_control_content(&projected[0], "metadata.json")
            .ok_or_else(|| io::Error::other("metadata control missing"))?;
        let document: serde_json::Value =
            serde_json::from_str(&content).map_err(|error| io::Error::other(error.to_string()))?;
        assert_eq!(
            document["metadata"]["models_dev"]["future_field"],
            "retained"
        );
        assert_eq!(document["effective"]["limit_tokens"], 1_000_000);
        assert!(projected[0].cap.contains("attachment"));
        assert!(projected[0].cap.contains("stream"));
        Ok(())
    }

    #[test]
    fn project_models_keeps_unmapped_aggregator_models_unverified() -> io::Result<()> {
        let dir = tempdir()?;
        let config = ProviderConfig {
            name: None,
            base_url: "https://api.lmm.best/v1".to_owned(),
            default_model: None,
            models: vec!["deepseek-v4-flash-0731".to_owned()],
            model_limits: HashMap::new(),
            model_capabilities: HashMap::new(),
            enabled: true,
            formats: vec!["openai.chat".to_owned()],
            auth: Vec::new(),
            oauth: None,
        };

        let mut projected = Vec::new();
        project_models("lmm", &config, dir.path(), &mut projected);
        let model = &projected[0];

        assert_eq!(model.limit.tokens(), None);
        assert_eq!(model.recommended.tokens(), None);
        assert_eq!(model.compact.tokens(), None);
        assert_eq!(model.cap, "chat\nstream\n");
        let content = projected_control_content(model, "metadata.json")
            .ok_or_else(|| io::Error::other("metadata control missing"))?;
        let document: serde_json::Value =
            serde_json::from_str(&content).map_err(|error| io::Error::other(error.to_string()))?;
        assert_eq!(document["resolution"], "unverified");
        assert_eq!(document["metadata"]["provider"], "lmm");
        assert_eq!(document["metadata"]["attachment"], "unknown");
        assert_eq!(document["metadata"]["models_dev"], serde_json::Value::Null);
        Ok(())
    }
}
