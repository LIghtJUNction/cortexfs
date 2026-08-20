#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires an explicit models.dev live-network check"]
    fn models_dev_live_catalog_normalizes_an_arbitrary_record()
    -> Result<(), Box<dyn std::error::Error>> {
        let cache = tempfile::tempdir()?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let catalog = runtime.block_on(cortexfs_metadatas::MetadataCatalog::from_models_dev(
            cache.path(),
        ))?;
        let model = catalog
            .models()
            .find(|model| model.context_window_tokens.is_some() && model.models_dev_base.is_some())
            .ok_or("models.dev returned no bounded model")?;
        let key = format!("{}/{}", model.provider, model.id);
        assert!(model.models_dev.is_some());
        assert!(model.models_dev_base.is_some());
        assert!(catalog.base_model(&key).is_some());
        assert!(catalog.provider(&model.provider).is_some());
        assert!(
            model
                .sources
                .iter()
                .any(|source| source.publisher == "models.dev")
        );
        assert!(model.context_policy().recommended_tokens.is_some());
        Ok(())
    }
}
