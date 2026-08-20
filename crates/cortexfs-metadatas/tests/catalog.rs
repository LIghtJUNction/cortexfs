#[cfg(test)]
mod tests {
    use cortexfs_metadatas::{
        MetadataCatalog, MetadataError, MetadataSource, ModelMetadata, Support,
    };

    #[test]
    fn context_policy_separates_hard_recommended_and_compaction_limits() {
        let defaults = ModelMetadata::new("local", "default", "Default")
            .with_context(1_000_000)
            .context_policy();
        assert_eq!(defaults.recommended_tokens, Some(500_000));
        assert_eq!(defaults.compaction_threshold_tokens, Some(450_000));

        let explicit = ModelMetadata::new("local", "long", "Long")
            .with_context(1_000_000)
            .with_context_policy(262_144, 209_715)
            .context_policy();
        assert_eq!(explicit.max_tokens, Some(1_000_000));
        assert_eq!(explicit.recommended_tokens, Some(262_144));
        assert_eq!(explicit.compaction_threshold_tokens, Some(209_715));
    }

    #[test]
    fn custom_models_support_aliases_and_explicit_mapping() -> Result<(), MetadataError> {
        let mut catalog = MetadataCatalog::new();
        let model = ModelMetadata::new("local", "model-v1", "Local Model")
            .with_aliases(["latest", "stable"])
            .with_context(32_768)
            .with_capabilities(Support::Supported, Support::Unknown, Support::Supported)
            .with_source(MetadataSource::official(
                "local",
                "https://example.invalid/model",
            ));
        catalog.register(model)?;
        assert_eq!(catalog.canonical_key("latest"), Some("local/model-v1"));
        assert_eq!(
            catalog.resolve("stable").map(|item| item.id.as_str()),
            Some("model-v1")
        );
        catalog.register_alias("production", "local/model-v1")?;
        assert_eq!(catalog.canonical_key("production"), Some("local/model-v1"));
        Ok(())
    }

    #[test]
    fn alias_conflicts_are_transactional() {
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
    fn upstream_payload_round_trips_without_dropping_fields() -> serde_json::Result<()> {
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
}
