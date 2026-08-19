#[cfg(test)]
mod tests {
    use std::fs;

    use cortexfs_metadatas::{
        MetadataCatalog, MetadataError, MetadataSource, MetadataSourceError, Modality,
        ModelMetadata, Support,
    };

    #[test]
    fn builtins_expose_official_capabilities_and_aliases() {
        let catalog = MetadataCatalog::builtins();
        assert!(catalog.len() >= 20);
        let openai = catalog.resolve("gpt-5.6");
        assert!(openai.is_some());
        if let Some(openai) = openai {
            assert_eq!(openai.provider, "openai");
            assert!(openai.supports_input(Modality::Image));
            assert_eq!(openai.tools, Support::Supported);
            assert!(!openai.sources.is_empty());
            let policy = openai.context_policy();
            assert!(policy.max_tokens >= policy.recommended_tokens);
            assert!(policy.compaction_threshold_tokens <= policy.recommended_tokens);
        }
        assert_eq!(
            catalog
                .resolve_for("anthropic", "claude-haiku-4-5")
                .map(|model| model.id.as_str()),
            Some("claude-haiku-4-5-20251001")
        );
    }

    #[test]
    fn context_policy_separates_hard_recommended_and_compaction_limits() {
        let default_model = ModelMetadata::new("local", "default", "Default")
            .with_context(1_000_000)
            .context_policy();
        assert_eq!(default_model.recommended_tokens, Some(500_000));
        assert_eq!(default_model.compaction_threshold_tokens, Some(450_000));

        let model = ModelMetadata::new("local", "long", "Long")
            .with_context(1_000_000)
            .with_context_policy(262_144, 209_715);
        let policy = model.context_policy();
        assert_eq!(policy.max_tokens, Some(1_000_000));
        assert_eq!(policy.recommended_tokens, Some(262_144));
        assert_eq!(policy.compaction_threshold_tokens, Some(209_715));
    }

    #[test]
    fn deepseek_v4_builtin_records_keep_distinct_models_dev_facts() -> Result<(), &'static str> {
        let catalog = MetadataCatalog::builtins();
        let Some(flash) = catalog.resolve_for("deepseek", "deepseek-v4-flash") else {
            return Err("missing DeepSeek V4 Flash metadata");
        };
        let Some(pro) = catalog.resolve_for("deepseek", "deepseek-v4-pro") else {
            return Err("missing DeepSeek V4 Pro metadata");
        };
        assert_eq!(
            flash
                .models_dev
                .as_ref()
                .and_then(|value| value.get("family"))
                .and_then(serde_json::Value::as_str),
            Some("deepseek-flash")
        );
        assert_eq!(
            pro.models_dev
                .as_ref()
                .and_then(|value| value.get("family"))
                .and_then(serde_json::Value::as_str),
            Some("deepseek-thinking")
        );
        assert_eq!(pro.release_date.as_deref(), Some("2026-08-12"));
        assert_eq!(pro.open_weights, Support::Unsupported);
        assert!(
            pro.models_dev
                .as_ref()
                .is_some_and(|value| value.get("knowledge").is_none())
        );
        assert!(
            pro.sources
                .iter()
                .any(|source| source.publisher == "models.dev")
        );
        Ok(())
    }

    #[test]
    fn custom_models_support_many_aliases_and_explicit_mapping() {
        let mut catalog = MetadataCatalog::new();
        let model = ModelMetadata::new("local", "model-v1", "Local Model")
            .with_aliases(["latest", "stable"])
            .with_context(32_768)
            .with_capabilities(Support::Supported, Support::Unknown, Support::Supported)
            .with_source(MetadataSource::official(
                "local",
                "https://example.invalid/model",
            ));
        assert!(catalog.register(model).is_ok());
        assert_eq!(catalog.canonical_key("latest"), Some("local/model-v1"));
        assert_eq!(
            catalog.resolve("stable").map(|item| item.id.as_str()),
            Some("model-v1")
        );
        assert!(
            catalog
                .register_alias("production", "local/model-v1")
                .is_ok()
        );
        assert_eq!(catalog.canonical_key("production"), Some("local/model-v1"));
    }

    #[test]
    fn alias_conflicts_and_unknown_models_are_reported_transactionally() {
        let mut catalog = MetadataCatalog::new();
        assert!(
            catalog
                .register(ModelMetadata::new("local", "one", "One").with_aliases(["shared"]))
                .is_ok()
        );
        let result =
            catalog.register(ModelMetadata::new("local", "two", "Two").with_aliases(["shared"]));
        assert!(matches!(result, Err(MetadataError::AliasConflict(_))));
        assert_eq!(catalog.len(), 1);
        assert!(matches!(
            catalog.register_alias("missing", "local/nope"),
            Err(MetadataError::UnknownModel(_))
        ));
    }

    #[test]
    fn official_payload_round_trips_without_dropping_upstream_fields() -> serde_json::Result<()> {
        let payload = serde_json::json!({
            "description": "long context model",
            "reasoning_options": [{"type": "effort", "values": ["low", "max"]}],
            "cost": {"input": 1.25, "cache_write": 0.1},
            "future_field": {"preserved": true}
        });
        let model =
            ModelMetadata::new("provider", "model", "Model").with_models_dev(payload.clone());
        let encoded = serde_json::to_value(model)?;
        assert_eq!(encoded.get("models_dev"), Some(&payload));
        Ok(())
    }

    #[test]
    fn models_dev_optional_fields_and_zero_limits_remain_unknown()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = std::env::temp_dir().join(format!(
            "cortexfs-metadatas-optional-fields-{}",
            std::process::id()
        ));
        fs::create_dir_all(&directory)?;
        let raw = serde_json::json!({
            "id": "optional",
            "name": "Optional",
            "attachment": false,
            "reasoning": false,
            "tool_call": false,
            "modalities": {"input": ["text"], "output": ["text"]},
            "open_weights": false,
            "interleaved": false,
            "limit": {"context": 0, "output": 0},
            "future_field": {"kept": true}
        });
        let mut metadata = ModelMetadata::new("provider", "optional", "Optional")
            .with_capabilities(Support::Unsupported, Support::Unknown, Support::Unknown)
            .with_models_dev(raw.clone());
        metadata.attachment = Support::Unsupported;
        metadata.reasoning.support = Support::Unsupported;
        metadata.open_weights = Support::Unsupported;
        metadata.interleaved = Support::Unsupported;
        metadata = metadata.with_source(MetadataSource::official(
            "models.dev",
            "https://models.dev/api.json",
        ));
        let payload = serde_json::json!({
            "schema": cortexfs_metadatas::MODEL_METADATA_SCHEMA,
            "models": {"provider/optional": metadata}
        });
        fs::write(
            directory.join("model-metadata.json"),
            serde_json::to_vec(&payload)?,
        )?;
        let catalog = MetadataCatalog::from_cache(&directory)?;
        let model = catalog
            .resolve("provider/optional")
            .ok_or("missing model")?;
        assert_eq!(model.context_window_tokens, None);
        assert_eq!(model.max_output_tokens, None);
        assert_eq!(model.temperature, Support::Unknown);
        assert_eq!(model.models_dev.as_ref(), Some(&raw));
        fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn models_dev_cache_rejects_mismatched_hard_limit() -> Result<(), Box<dyn std::error::Error>> {
        let directory = std::env::temp_dir().join(format!(
            "cortexfs-metadatas-validation-{}",
            std::process::id()
        ));
        fs::create_dir_all(&directory)?;
        let metadata = ModelMetadata::new("deepseek", "checked", "Checked")
            .with_context(8)
            .with_max_output(4)
            .with_models_dev(serde_json::json!({
                "id": "checked",
                "name": "Checked",
                "attachment": false,
                "reasoning": false,
                "tool_call": false,
                "modalities": {"input": ["text"], "output": ["text"]},
                "open_weights": false,
                "limit": {"context": 16, "output": 4}
            }))
            .with_source(MetadataSource::official(
                "models.dev",
                "https://models.dev/api.json",
            ));
        let payload = serde_json::json!({
            "schema": cortexfs_metadatas::MODEL_METADATA_SCHEMA,
            "models": {"deepseek/checked": metadata}
        });
        fs::write(
            directory.join("model-metadata.json"),
            serde_json::to_vec(&payload)?,
        )?;
        let result = MetadataCatalog::from_cache(&directory);
        let invalid = matches!(
            result,
            Err(MetadataSourceError::CacheInvalid(_, reason))
                if reason.contains("context limit")
        );
        fs::remove_dir_all(directory)?;
        assert!(invalid);
        Ok(())
    }

    #[test]
    fn verified_models_dev_facts_replace_stale_builtin_capabilities()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory =
            std::env::temp_dir().join(format!("cortexfs-metadatas-merge-{}", std::process::id()));
        fs::create_dir_all(&directory)?;
        let mut metadata = ModelMetadata::new("openai", "gpt-5.6-sol", "Verified")
            .with_context(1)
            .with_max_output(1)
            .with_modalities(&[Modality::Text], &[Modality::Text])
            .with_capabilities(Support::Unsupported, Support::Unknown, Support::Unknown)
            .with_models_dev(serde_json::json!({
                "id": "gpt-5.6-sol",
                "name": "Verified",
                "attachment": false,
                "reasoning": false,
                "tool_call": false,
                "modalities": {"input": ["text"], "output": ["text"]},
                "open_weights": false,
                "limit": {"context": 1, "output": 1}
            }))
            .with_source(MetadataSource::official(
                "models.dev",
                "https://models.dev/api.json",
            ));
        metadata.reasoning.support = Support::Unsupported;
        metadata.attachment = Support::Unsupported;
        metadata.open_weights = Support::Unsupported;
        let payload = serde_json::json!({
            "schema": cortexfs_metadatas::MODEL_METADATA_SCHEMA,
            "models": {"openai/gpt-5.6-sol": metadata}
        });
        fs::write(
            directory.join("model-metadata.json"),
            serde_json::to_vec(&payload)?,
        )?;
        let catalog = MetadataCatalog::from_cache_or_builtins(&directory);
        let model = catalog
            .resolve("openai/gpt-5.6-sol")
            .ok_or_else(|| std::io::Error::other("merged model is missing"))?;
        assert_eq!(model.input_modalities, vec![Modality::Text]);
        assert_eq!(model.tools, Support::Unsupported);
        assert_eq!(model.reasoning.support, Support::Unsupported);
        assert_eq!(model.structured_output, Support::Supported);
        fs::remove_dir_all(directory)?;
        Ok(())
    }
}
