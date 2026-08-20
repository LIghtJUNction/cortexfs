#[cfg(test)]
mod tests {
    use std::fs;

    use cortexfs_metadatas::{
        MODEL_METADATA_SCHEMA, MetadataCatalog, MetadataSourceError, Support,
    };

    fn model(context: &serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "id": "dynamic", "name": "Dynamic", "attachment": false,
            "reasoning": false, "tool_call": false, "open_weights": false,
            "modalities": {"input": ["text"], "output": ["text"]},
            "interleaved": false,
            "limit": {"context": context, "output": 0},
            "future_field": {"kept": true}
        })
    }

    fn raw_catalog(context: &serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "providers": {"dynamic": {
                "id": "dynamic", "name": "Dynamic Provider",
                "npm": "@ai-sdk/openai-compatible", "env": ["DYNAMIC_API_KEY"],
                "doc": "https://example.invalid/docs", "api": "https://example.invalid/v1",
                "models": {"dynamic": model(context)}
            }},
            "models": {"dynamic/dynamic": {
                "id": "dynamic/dynamic", "name": "Dynamic",
                "benchmarks": [{"name": "Example", "score": 90}],
                "weights": [{"label": "Weights", "url": "https://example.invalid/model"}]
            }}
        })
    }

    fn write_cache(path: &std::path::Path, catalog: &serde_json::Value) -> std::io::Result<()> {
        let payload = serde_json::json!({
            "schema": MODEL_METADATA_SCHEMA,
            "observed_on": "Thu, 20 Aug 2026 00:00:00 GMT",
            "catalog": catalog
        });
        fs::write(
            path.join("model-metadata.json"),
            serde_json::to_vec(&payload).map_err(std::io::Error::other)?,
        )
    }

    #[test]
    fn valid_cache_builds_models_and_preserves_all_views() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempfile::tempdir()?;
        write_cache(directory.path(), &raw_catalog(&serde_json::json!(0)))?;
        let catalog = MetadataCatalog::from_cache(directory.path())?;
        let metadata = catalog
            .resolve_for("dynamic", "dynamic")
            .ok_or_else(|| std::io::Error::other("dynamic model is missing"))?;
        assert_eq!(metadata.context_window_tokens, None);
        assert_eq!(metadata.temperature, Support::Unknown);
        assert!(
            metadata
                .models_dev
                .as_ref()
                .is_some_and(|raw| raw.get("future_field").is_some())
        );
        assert!(
            metadata
                .models_dev_base
                .as_ref()
                .is_some_and(|raw| raw.get("benchmarks").is_some())
        );
        assert_eq!(
            catalog
                .provider("dynamic")
                .and_then(|raw| raw.get("api"))
                .and_then(serde_json::Value::as_str),
            Some("https://example.invalid/v1")
        );
        assert!(catalog.base_model("dynamic/dynamic").is_some());
        assert_eq!(
            metadata
                .sources
                .first()
                .map(|source| source.observed_on.as_str()),
            Some("Thu, 20 Aug 2026 00:00:00 GMT")
        );
        Ok(())
    }

    #[test]
    fn missing_cache_does_not_invent_provider_models() -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        assert!(MetadataCatalog::from_cache_or_empty(directory.path()).is_empty());
        Ok(())
    }

    #[test]
    fn cache_rejects_invalid_upstream_limits() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        write_cache(
            directory.path(),
            &raw_catalog(&serde_json::json!("invalid")),
        )?;
        let invalid = matches!(
            MetadataCatalog::from_cache(directory.path()),
            Err(MetadataSourceError::CacheInvalid(_, reason)) if reason.contains("limit.context")
        );
        assert!(invalid);
        Ok(())
    }
}
