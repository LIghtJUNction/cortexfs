#[cfg(test)]
mod tests {
    use cortexfs_metadatas::{
        MetadataCatalog, MetadataError, MetadataSource, Modality, ModelMetadata, Support,
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
        }
        assert_eq!(
            catalog
                .resolve_for("anthropic", "claude-haiku-4-5")
                .map(|model| model.id.as_str()),
            Some("claude-haiku-4-5-20251001")
        );
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
}
