use crate::{MetadataError, ModelMetadata};
use std::collections::BTreeMap;

/// Runtime catalog with canonical identities and many-to-one aliases.
#[derive(Clone, Debug, Default)]
pub struct MetadataCatalog {
    models: BTreeMap<String, ModelMetadata>,
    aliases: BTreeMap<String, String>,
}

impl MetadataCatalog {
    /// Creates an empty application-owned catalog.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a catalog containing the checked-in official snapshot.
    #[must_use]
    pub fn builtins() -> Self {
        let mut catalog = Self::new();
        for model in crate::builtin_models() {
            let registered = catalog.register(model).is_ok();
            debug_assert!(registered);
        }
        catalog
    }

    /// Registers a canonical model and its provider-local aliases.
    pub fn register(&mut self, metadata: ModelMetadata) -> Result<(), MetadataError> {
        if metadata.provider.trim().is_empty() {
            return Err(MetadataError::EmptyProvider);
        }
        if metadata.id.trim().is_empty() {
            return Err(MetadataError::EmptyModelId);
        }
        let key = qualified(&metadata.provider, &metadata.id);
        if self.models.contains_key(&key) {
            return Err(MetadataError::DuplicateModel(key));
        }
        let aliases = metadata.aliases.clone();
        let mut next = self.clone();
        next.models.insert(key.clone(), metadata);
        for alias in aliases {
            next.add_provider_alias(&key, &alias)?;
        }
        *self = next;
        Ok(())
    }

    pub(crate) fn add_provider_alias(
        &mut self,
        key: &str,
        alias: &str,
    ) -> Result<(), MetadataError> {
        if alias.trim().is_empty() {
            return Err(MetadataError::EmptyAlias);
        }
        self.insert_alias(qualified_from_key(key, alias), key.to_owned())?;
        if !self.models.contains_key(alias) && !self.aliases.contains_key(alias) {
            self.aliases.insert(alias.to_owned(), key.to_owned());
        }
        Ok(())
    }

    pub(crate) fn insert_alias(&mut self, alias: String, key: String) -> Result<(), MetadataError> {
        if let Some(existing) = self.models.get(&alias)
            && qualified(&existing.provider, &existing.id) != key
        {
            return Err(MetadataError::AliasConflict(alias));
        }
        if let Some(existing) = self.aliases.get(&alias)
            && existing != &key
        {
            return Err(MetadataError::AliasConflict(alias));
        }
        self.aliases.insert(alias, key);
        Ok(())
    }

    pub(crate) fn canonical_reference(&self, reference: &str) -> Option<&str> {
        self.models
            .get_key_value(reference)
            .map(|(key, _)| key.as_str())
            .or_else(|| self.aliases.get(reference).map(String::as_str))
    }

    pub(crate) fn model_at(&self, key: &str) -> Option<&ModelMetadata> {
        self.models.get(key)
    }

    pub(crate) fn has_model(&self, key: &str) -> bool {
        self.models.contains_key(key)
    }

    pub(crate) fn all_models(&self) -> impl Iterator<Item = &ModelMetadata> {
        self.models.values()
    }

    pub(crate) fn model_count(&self) -> usize {
        self.models.len()
    }
}

pub(crate) fn qualified(provider: &str, model: &str) -> String {
    format!("{provider}/{model}")
}

fn qualified_from_key(key: &str, alias: &str) -> String {
    key.split_once('/').map_or_else(
        || alias.to_owned(),
        |(provider, _)| qualified(provider, alias),
    )
}
