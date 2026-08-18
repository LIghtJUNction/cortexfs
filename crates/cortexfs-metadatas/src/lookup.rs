use crate::catalog::qualified;
use crate::{MetadataCatalog, MetadataError};

impl MetadataCatalog {
    /// Adds an explicit alias to a canonical `provider/model` key.
    pub fn register_alias(
        &mut self,
        alias: impl Into<String>,
        canonical: impl Into<String>,
    ) -> Result<(), MetadataError> {
        let alias = alias.into();
        let canonical = canonical.into();
        if alias.trim().is_empty() {
            return Err(MetadataError::EmptyAlias);
        }
        if !self.has_model(&canonical) {
            return Err(MetadataError::UnknownModel(canonical));
        }
        self.insert_alias(alias, canonical)
    }

    /// Adds an alias scoped to one provider, then globally if it is unique.
    pub fn register_provider_alias(
        &mut self,
        provider: &str,
        alias: &str,
        model_id: &str,
    ) -> Result<(), MetadataError> {
        let key = qualified(provider, model_id);
        self.add_provider_alias(&key, alias)
    }

    /// Resolves either a canonical key or an alias.
    #[must_use]
    pub fn resolve(&self, reference: &str) -> Option<&crate::ModelMetadata> {
        self.canonical_reference(reference)
            .and_then(|key| self.model_at(key))
    }

    /// Resolves a short model ID within a provider before a global alias.
    #[must_use]
    pub fn resolve_for(&self, provider: &str, reference: &str) -> Option<&crate::ModelMetadata> {
        let key = qualified(provider, reference);
        self.resolve(&key).or_else(|| self.resolve(reference))
    }

    /// Returns the canonical `provider/model` key for a reference.
    #[must_use]
    pub fn canonical_key(&self, reference: &str) -> Option<&str> {
        self.canonical_reference(reference)
    }

    /// Iterates over canonical model records in deterministic key order.
    pub fn models(&self) -> impl Iterator<Item = &crate::ModelMetadata> {
        self.all_models()
    }

    /// Number of canonical model records.
    #[must_use]
    pub fn len(&self) -> usize {
        self.model_count()
    }

    /// Whether no canonical records are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.model_count() == 0
    }
}
