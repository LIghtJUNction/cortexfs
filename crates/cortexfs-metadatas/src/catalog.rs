use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, from_slice, to_vec};

use crate::remote::RemoteCatalog;
use crate::source::{
    CachedMetadataCatalog, MAX_CACHE_BYTES, MAX_CACHED_MODELS, MODEL_METADATA_CACHE_FILE,
    MODEL_METADATA_SCHEMA, MODELS_DEV_ENDPOINT,
};
use crate::validate::{is_models_dev_record, validate_models_dev_record};
use crate::{
    MetadataError, MetadataSource, MetadataSourceError, Modality, ModelMetadata, ReasoningMetadata,
    Support,
};

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

    /// Loads a validated cache payload.
    pub fn from_cache(cache_dir: &Path) -> Result<Self, MetadataSourceError> {
        let path = catalog_cache_path(cache_dir);
        let models = read_cached_models(&path)?;
        catalog_from_models(&models, &path)
    }

    /// Loads cached metadata if available, otherwise returns the checked-in snapshot.
    #[must_use]
    pub fn from_cache_or_builtins(cache_dir: &Path) -> Self {
        let Some(cached) = Self::from_cache(cache_dir).ok() else {
            return Self::builtins();
        };
        let mut catalog = Self::builtins();
        for metadata in cached.models.into_values() {
            let key = qualified(&metadata.provider, &metadata.id);
            let target = catalog
                .canonical_reference(&key)
                .map(str::to_owned)
                .unwrap_or(key);
            if let Some(base) = catalog.models.get(&target).cloned() {
                catalog
                    .models
                    .insert(target, merge_metadata(base, metadata));
            } else {
                let _ignored = catalog.register(metadata);
            }
        }
        catalog
    }

    /// Resolves and publishes remote metadata into cache.
    pub async fn from_models_dev(cache_dir: &Path) -> Result<Self, MetadataSourceError> {
        let response = crate::remote::fetch().await?;
        let models = remote_model_map(response)?;
        write_cache(cache_dir, &models)?;
        catalog_from_models(&models, &catalog_cache_path(cache_dir))
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
    pub fn resolve(&self, reference: &str) -> Option<&ModelMetadata> {
        self.canonical_reference(reference)
            .and_then(|key| self.model_at(key))
    }

    /// Resolves a short model ID within a provider before a global alias.
    #[must_use]
    pub fn resolve_for(&self, provider: &str, reference: &str) -> Option<&ModelMetadata> {
        let key = qualified(provider, reference);
        self.resolve(&key).or_else(|| self.resolve(reference))
    }

    /// Returns the canonical `provider/model` key for a reference.
    #[must_use]
    pub fn canonical_key(&self, reference: &str) -> Option<&str> {
        self.canonical_reference(reference)
    }

    /// Iterates over canonical model records in deterministic key order.
    pub fn models(&self) -> impl Iterator<Item = &ModelMetadata> {
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

pub(crate) fn catalog_cache_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(MODEL_METADATA_CACHE_FILE)
}

fn merge_metadata(mut base: ModelMetadata, overlay: ModelMetadata) -> ModelMetadata {
    let authoritative = is_models_dev_record(&overlay);
    if !overlay.name.trim().is_empty() {
        base.name = overlay.name;
    }
    base.status = overlay.status;
    base.context_window_tokens = overlay.context_window_tokens.or(base.context_window_tokens);
    base.recommended_context_tokens = overlay
        .recommended_context_tokens
        .or(base.recommended_context_tokens);
    base.compaction_threshold_tokens = overlay
        .compaction_threshold_tokens
        .or(base.compaction_threshold_tokens);
    base.max_output_tokens = overlay.max_output_tokens.or(base.max_output_tokens);
    if authoritative {
        base.tools = overlay.tools;
        base.structured_output = overlay.structured_output;
        base.streaming = overlay.streaming;
        base.attachment = overlay.attachment;
        base.temperature = overlay.temperature;
        base.open_weights = overlay.open_weights;
        base.interleaved = overlay.interleaved;
        base.description = overlay.description;
        base.family = overlay.family;
        base.knowledge = overlay.knowledge;
        base.release_date = overlay.release_date;
        base.last_updated = overlay.last_updated;
        base.input_modalities = overlay.input_modalities;
        base.output_modalities = overlay.output_modalities;
        base.reasoning = overlay.reasoning;
    } else {
        base.tools = overlay_support(base.tools, overlay.tools);
        base.structured_output = overlay_support(base.structured_output, overlay.structured_output);
        base.streaming = overlay_support(base.streaming, overlay.streaming);
        base.attachment = overlay_support(base.attachment, overlay.attachment);
        base.temperature = overlay_support(base.temperature, overlay.temperature);
        base.open_weights = overlay_support(base.open_weights, overlay.open_weights);
        base.interleaved = overlay_support(base.interleaved, overlay.interleaved);
        base.description = overlay.description.or(base.description);
        base.family = overlay.family.or(base.family);
        base.knowledge = overlay.knowledge.or(base.knowledge);
        base.release_date = overlay.release_date.or(base.release_date);
        base.last_updated = overlay.last_updated.or(base.last_updated);
        if overlay.input_modalities != [Modality::Text] || base.input_modalities.is_empty() {
            base.input_modalities = overlay.input_modalities;
        }
        if overlay.output_modalities != [Modality::Text] || base.output_modalities.is_empty() {
            base.output_modalities = overlay.output_modalities;
        }
        if overlay.reasoning.support != Support::Unknown {
            base.reasoning = overlay.reasoning;
        }
    }
    for alias in overlay.aliases {
        if !base.aliases.contains(&alias) {
            base.aliases.push(alias);
        }
    }
    base.models_dev = overlay.models_dev.or(base.models_dev);
    base.sources.extend(overlay.sources);
    base
}

fn overlay_support(base: Support, overlay: Support) -> Support {
    if overlay == Support::Unknown {
        base
    } else {
        overlay
    }
}

fn remote_model_map(
    response: RemoteCatalog,
) -> Result<BTreeMap<String, ModelMetadata>, MetadataSourceError> {
    let mut models = BTreeMap::new();
    let RemoteCatalog { raw } = response;
    let providers = raw.as_object().ok_or_else(|| {
        MetadataSourceError::InvalidRemote("models.dev response is not an object".to_owned())
    })?;
    for (provider_key, provider) in providers {
        if !is_object_token(provider_key) {
            return Err(MetadataSourceError::InvalidRemote(
                "provider id is not canonical".to_owned(),
            ));
        }
        let provider_object = provider.as_object().ok_or_else(|| {
            MetadataSourceError::InvalidRemote("provider metadata is not an object".to_owned())
        })?;
        if provider_object.get("id").and_then(Value::as_str) != Some(provider_key) {
            return Err(MetadataSourceError::InvalidRemote(
                "provider map key does not match provider id".to_owned(),
            ));
        }
        let provider_models = provider_object
            .get("models")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                MetadataSourceError::InvalidRemote("provider models are not an object".to_owned())
            })?;
        for (model_key, model) in provider_models {
            if !is_model_id(model_key) {
                return Err(MetadataSourceError::InvalidRemote(
                    "model id contains an unsafe character".to_owned(),
                ));
            }
            let entry = model_metadata_from_models_dev(provider_key, provider, model_key, model)?;
            validate_models_dev_record(&entry)
                .map_err(|error| MetadataSourceError::InvalidRemote(error.to_string()))?;
            let canonical = qualified(&entry.provider, &entry.id);
            if models.contains_key(&canonical) {
                return Err(MetadataSourceError::InvalidRemote(
                    "duplicate canonical model key".to_owned(),
                ));
            }
            models.insert(canonical, entry);
        }
    }
    if models.is_empty() {
        return Err(MetadataSourceError::InvalidRemote(
            "no usable model metadata in response".to_owned(),
        ));
    }
    if models.len() > MAX_CACHED_MODELS {
        return Err(MetadataSourceError::InvalidRemote(
            "models.dev returned more records than cache policy allows".to_owned(),
        ));
    }
    Ok(models)
}

fn model_metadata_from_models_dev(
    provider_key: &str,
    provider: &Value,
    model_key: &str,
    raw_value: &Value,
) -> Result<ModelMetadata, MetadataSourceError> {
    let raw_model = raw_model(raw_value, model_key)?;
    let context = raw_u32(raw_model, &["limit", "context"])?;
    let output = raw_u32(raw_model, &["limit", "output"])?;
    let tool_call = raw_bool(raw_model, "tool_call")?;
    let reasoning = raw_bool(raw_model, "reasoning")?;
    let input = raw_modalities(raw_model, "input")?;
    let output_modalities = raw_modalities(raw_model, "output")?;
    let mut metadata = ModelMetadata::new(
        provider_key,
        model_key,
        raw_string(raw_model, "name")?.to_owned(),
    )
    .with_modalities(
        &modalities_from(&input),
        &modalities_from(&output_modalities),
    )
    .with_capabilities(
        if tool_call {
            Support::Supported
        } else {
            Support::Unsupported
        },
        Support::Unknown,
        Support::Unknown,
    )
    .with_models_dev(raw_model.clone());
    if context > 0 {
        metadata = metadata.with_context(context);
    }
    if output > 0 {
        metadata = metadata.with_max_output(output);
    }

    metadata.description = string_field(raw_model, "description");
    metadata.family = string_field(raw_model, "family");
    metadata.knowledge = string_field(raw_model, "knowledge");
    metadata.release_date = string_field(raw_model, "release_date");
    metadata.last_updated = string_field(raw_model, "last_updated");
    metadata.attachment = bool_support(raw_model, "attachment");
    metadata.temperature = bool_support(raw_model, "temperature");
    metadata.streaming = bool_support(raw_model, "streaming");
    metadata.open_weights = bool_support(raw_model, "open_weights");
    metadata.interleaved = interleaved_support(raw_model);
    metadata.structured_output = bool_support(raw_model, "structured_output");
    metadata.status = status_from(raw_model);

    metadata.reasoning = ReasoningMetadata {
        support: if reasoning {
            Support::Supported
        } else {
            Support::Unsupported
        },
        levels: reasoning_levels(raw_model),
        parameter: if reasoning {
            Some("reasoning".to_owned())
        } else {
            None
        },
        default_level: None,
        max_tokens: None,
    };

    metadata.sources.push(MetadataSource::official(
        "models.dev",
        format!("{MODELS_DEV_ENDPOINT}/api.json"),
    ));
    let provider_object = provider.as_object().ok_or_else(|| {
        MetadataSourceError::InvalidRemote("provider metadata is not an object".to_owned())
    })?;
    let provider_name = provider_object
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(provider_key);
    let provider_doc = provider_object
        .get("doc")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !provider_doc.trim().is_empty() {
        metadata
            .sources
            .push(MetadataSource::official(provider_name, provider_doc));
    }

    Ok(metadata)
}

fn raw_model<'a>(value: &'a Value, model: &str) -> Result<&'a Value, MetadataSourceError> {
    let object = value.as_object().ok_or_else(|| {
        MetadataSourceError::InvalidRemote("model metadata is not an object".to_owned())
    })?;
    if object.get("id").and_then(Value::as_str) != Some(model) {
        return Err(MetadataSourceError::InvalidRemote(
            "models.dev model id does not match map key".to_owned(),
        ));
    }
    for field in [
        "name",
        "attachment",
        "reasoning",
        "tool_call",
        "modalities",
        "open_weights",
        "limit",
    ] {
        if !object.contains_key(field) {
            return Err(MetadataSourceError::InvalidRemote(format!(
                "models.dev model is missing {field}"
            )));
        }
    }
    Ok(value)
}

fn raw_string<'a>(value: &'a Value, field: &str) -> Result<&'a str, MetadataSourceError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| MetadataSourceError::InvalidRemote(format!("models.dev {field} is invalid")))
}

fn raw_bool(value: &Value, field: &str) -> Result<bool, MetadataSourceError> {
    value
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| MetadataSourceError::InvalidRemote(format!("models.dev {field} is invalid")))
}

fn raw_u32(value: &Value, path: &[&str]) -> Result<u32, MetadataSourceError> {
    let value = path
        .iter()
        .try_fold(value, |value, field| value.get(*field));
    value
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| {
            MetadataSourceError::InvalidRemote(format!("models.dev {} is invalid", path.join(".")))
        })
}

fn raw_modalities(value: &Value, field: &str) -> Result<Vec<String>, MetadataSourceError> {
    value
        .get("modalities")
        .and_then(Value::as_object)
        .and_then(|value| value.get(field))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            MetadataSourceError::InvalidRemote(format!("models.dev modalities.{field} is invalid"))
        })?
        .iter()
        .map(|value| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                MetadataSourceError::InvalidRemote(format!(
                    "models.dev modalities.{field} contains a non-string"
                ))
            })
        })
        .collect()
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_owned)
}

fn bool_support(value: &Value, field: &str) -> Support {
    match value.get(field).and_then(Value::as_bool) {
        Some(true) => Support::Supported,
        Some(false) => Support::Unsupported,
        None => Support::Unknown,
    }
}

fn interleaved_support(value: &Value) -> Support {
    let Some(value) = value.get("interleaved") else {
        return Support::Unknown;
    };
    if value.is_null() {
        Support::Unknown
    } else if value.as_bool() == Some(false) {
        Support::Unsupported
    } else {
        Support::Supported
    }
}

fn status_from(value: &Value) -> crate::ModelStatus {
    match value.get("status").and_then(Value::as_str) {
        Some("alpha" | "beta") => crate::ModelStatus::Preview,
        Some("deprecated") => crate::ModelStatus::Deprecated,
        _ => crate::ModelStatus::Active,
    }
}

fn reasoning_levels(value: &Value) -> Vec<String> {
    value
        .get("reasoning_options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|option| option.get("type").and_then(Value::as_str) == Some("effort"))
        .flat_map(|option| {
            option
                .get("values")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn catalog_from_models(
    models: &BTreeMap<String, ModelMetadata>,
    path: &Path,
) -> Result<MetadataCatalog, MetadataSourceError> {
    if models.is_empty() {
        return Err(MetadataSourceError::CacheInvalid(
            path.to_owned(),
            "empty catalog".to_owned(),
        ));
    }

    let mut catalog = MetadataCatalog::new();
    for (key, metadata) in models {
        if is_models_dev_record(metadata) {
            validate_models_dev_record(metadata).map_err(|error| {
                MetadataSourceError::CacheInvalid(path.to_owned(), error.to_string())
            })?;
        }
        let entry_key = qualified(&metadata.provider, &metadata.id);
        if entry_key != *key {
            return Err(MetadataSourceError::CacheInvalid(
                path.to_owned(),
                "metadata key does not match provider/model identity".to_owned(),
            ));
        }
        if catalog.models.contains_key(key) || catalog.aliases.contains_key(key) {
            return Err(MetadataSourceError::InvalidRemote(
                "duplicate canonical model key".to_owned(),
            ));
        }
        let aliases = metadata.aliases.clone();
        catalog.models.insert(key.clone(), metadata.clone());
        for alias in aliases {
            catalog
                .add_provider_alias(key, &alias)
                .map_err(|error| MetadataSourceError::InvalidRemote(error.to_string()))?;
        }
    }
    Ok(catalog)
}

fn write_cache(
    cache_dir: &Path,
    models: &BTreeMap<String, ModelMetadata>,
) -> Result<(), MetadataSourceError> {
    let path = catalog_cache_path(cache_dir);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| MetadataSourceError::CacheIoWrite {
            path: path.clone(),
            source,
        })?;
    }

    if let Ok(meta) = path.symlink_metadata()
        && !meta.is_file()
    {
        return Err(MetadataSourceError::CacheInvalid(
            path,
            "cache path is not a regular file".to_owned(),
        ));
    }

    let cache = CachedMetadataCatalog {
        schema: MODEL_METADATA_SCHEMA.to_owned(),
        models: models.clone(),
    };
    let mut bytes = to_vec(&cache).map_err(|source| MetadataSourceError::CacheCorrupt {
        path: path.clone(),
        source,
    })?;
    if bytes.len() > MAX_CACHE_BYTES {
        return Err(MetadataSourceError::CacheOversize(bytes.len()));
    }
    bytes.push(b'\n');

    let tmp_path = path.with_extension("tmp");
    {
        let mut file =
            File::create(&tmp_path).map_err(|source| MetadataSourceError::CacheIoWrite {
                path: tmp_path.clone(),
                source,
            })?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|source| MetadataSourceError::CacheIoWrite {
                path: tmp_path.clone(),
                source,
            })?;
    }

    fs::rename(&tmp_path, &path).map_err(|source| {
        let _ignored = fs::remove_file(&tmp_path);
        MetadataSourceError::CacheIoWrite { path, source }
    })
}

fn read_cached_models(path: &Path) -> Result<BTreeMap<String, ModelMetadata>, MetadataSourceError> {
    let metadata = path
        .symlink_metadata()
        .map_err(|source| match source.kind() {
            std::io::ErrorKind::NotFound => MetadataSourceError::CacheMissing,
            _ => MetadataSourceError::CacheIoRead {
                path: path.to_owned(),
                source,
            },
        })?;

    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(MetadataSourceError::CacheInvalid(
            path.to_owned(),
            "cache path is not a regular file".to_owned(),
        ));
    }
    let length = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if length > MAX_CACHE_BYTES {
        return Err(MetadataSourceError::CacheOversize(length));
    }

    let raw = fs::read(path).map_err(|source| MetadataSourceError::CacheIoRead {
        path: path.to_owned(),
        source,
    })?;

    let cache: CachedMetadataCatalog =
        from_slice(&raw).map_err(|source| MetadataSourceError::CacheCorrupt {
            path: path.to_owned(),
            source,
        })?;

    if cache.schema != MODEL_METADATA_SCHEMA {
        return Err(MetadataSourceError::CacheInvalid(
            path.to_owned(),
            "unsupported cache schema".to_owned(),
        ));
    }
    if cache.models.is_empty() {
        return Err(MetadataSourceError::CacheInvalid(
            path.to_owned(),
            "empty metadata cache".to_owned(),
        ));
    }
    if cache.models.len() > MAX_CACHED_MODELS {
        return Err(MetadataSourceError::CacheInvalid(
            path.to_owned(),
            "cached catalog has too many records".to_owned(),
        ));
    }

    let mut models = BTreeMap::new();
    for (key, metadata) in cache.models {
        let (provider, model_id) = key
            .split_once('/')
            .filter(|&(provider, model)| !provider.is_empty() && !model.is_empty())
            .ok_or_else(|| {
                MetadataSourceError::CacheInvalid(
                    path.to_owned(),
                    "invalid model key format".to_owned(),
                )
            })?;
        if !is_object_token(provider) || !is_model_id(model_id) {
            return Err(MetadataSourceError::CacheInvalid(
                path.to_owned(),
                "invalid model key token".to_owned(),
            ));
        }
        if metadata.provider != provider || metadata.id != model_id {
            return Err(MetadataSourceError::CacheInvalid(
                path.to_owned(),
                "cache metadata key mismatch".to_owned(),
            ));
        }
        models.insert(key, metadata);
    }

    Ok(models)
}

fn modalities_from(modalities: &[String]) -> Vec<Modality> {
    let mut mapped = modalities
        .iter()
        .filter_map(|name| match name.as_str() {
            "text" => Some(Modality::Text),
            "image" => Some(Modality::Image),
            "audio" => Some(Modality::Audio),
            "video" => Some(Modality::Video),
            "pdf" => Some(Modality::Pdf),
            _ => None,
        })
        .collect::<Vec<_>>();

    if mapped.is_empty() {
        mapped.push(Modality::Text);
    }
    mapped
}

fn is_object_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn is_model_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| !character.is_ascii_control() && !character.is_whitespace())
}

fn qualified(provider: &str, model: &str) -> String {
    format!("{provider}/{model}")
}

fn qualified_from_key(key: &str, alias: &str) -> String {
    key.split_once('/').map_or_else(
        || alias.to_owned(),
        |(provider, _)| qualified(provider, alias),
    )
}
