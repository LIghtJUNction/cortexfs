use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cortexfs_metadatas::{MetadataCatalog, MetadataSourceError};
use serde::{Deserialize, Serialize};

use crate::support::plain::{create_plain_dir, read_small_text_file};
use crate::{AtomicReplaceOutcome, FuseError, atomic_replace_text_outcome, is_object_name};

const CACHE_FILE: &str = "model-limits.json";
const CACHE_SCHEMA: &str = "cortexfs.model-limits/v1";
const MAX_CACHE_BYTES: u64 = 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 4096;
const FETCH_TIMEOUT: Duration = Duration::from_secs(35);

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ModelLimitCache {
    schema: String,
    models: BTreeMap<String, u32>,
}

pub(crate) fn model_limit_cache_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(CACHE_FILE)
}

pub(crate) fn cached_model_limits(cache_dir: &Path) -> BTreeMap<String, u32> {
    let Ok(content) = read_small_text_file(&model_limit_cache_path(cache_dir), MAX_CACHE_BYTES)
    else {
        return BTreeMap::new();
    };
    let Ok(cache) = serde_json::from_str::<ModelLimitCache>(&content) else {
        return BTreeMap::new();
    };
    if validate_cache(&cache).is_err() {
        return BTreeMap::new();
    }
    cache.models
}

pub(crate) fn refresh_model_limit_cache(cache_dir: &Path) -> Result<(), FuseError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_error| FuseError::Io)?;
    runtime.block_on(refresh_timed(
        cache_dir,
        FETCH_TIMEOUT,
        MetadataCatalog::from_models_dev(cache_dir),
    ))
}

/// Applies a cache refresh with a bounded timeout.
async fn refresh_timed<F>(cache_dir: &Path, timeout: Duration, fetch: F) -> Result<(), FuseError>
where
    F: Future<Output = Result<MetadataCatalog, MetadataSourceError>>,
{
    let catalog = tokio::time::timeout(timeout, fetch)
        .await
        .map_err(|_error| FuseError::Io)?
        .map_err(|error| FuseError::from_metadata_source(&error))?;
    publish_cache(cache_dir, &catalog)
}

#[cfg(test)]
/// Refreshes provider catalog cache from a preloaded response.
async fn refresh_from<F>(cache_dir: &Path, fetch: F) -> Result<(), FuseError>
where
    F: Future<Output = Result<MetadataCatalog, MetadataSourceError>>,
{
    let catalog = fetch
        .await
        .map_err(|error| FuseError::from_metadata_source(&error))?;
    publish_cache(cache_dir, &catalog)
}

fn publish_cache(cache_dir: &Path, catalog: &MetadataCatalog) -> Result<(), FuseError> {
    let cache = cache_from_catalog(catalog)?;
    let content = serde_json::to_string(&cache).map_err(|_error| FuseError::Io)? + "\n";
    if u64::try_from(content.len()).map_err(|_error| FuseError::TooLarge)? > MAX_CACHE_BYTES {
        return Err(FuseError::TooLarge);
    }
    create_plain_dir(cache_dir).map_err(|_error| FuseError::Io)?;
    match atomic_replace_text_outcome(&model_limit_cache_path(cache_dir), &content) {
        Ok(AtomicReplaceOutcome::Synced | AtomicReplaceOutcome::PublishedUnsynced(_)) => Ok(()),
        Err(_error) => Err(FuseError::Io),
    }
}

fn cache_from_catalog(catalog: &MetadataCatalog) -> Result<ModelLimitCache, FuseError> {
    let mut models = BTreeMap::new();

    for model in catalog.models() {
        let Some(context_window) = model.context_window_tokens else {
            continue;
        };
        if context_window == 0 {
            continue;
        }
        if models.len() >= MAX_CACHE_ENTRIES {
            return Err(FuseError::TooLarge);
        }
        if !is_object_name(&model.provider) || !is_object_name(&model.id) {
            continue;
        }
        models.insert(format!("{}/{}", model.provider, model.id), context_window);
    }

    if models.is_empty() {
        return Err(FuseError::InvalidContent);
    }

    let cache = ModelLimitCache {
        schema: CACHE_SCHEMA.to_owned(),
        models,
    };
    validate_cache(&cache)?;
    Ok(cache)
}

fn validate_cache(cache: &ModelLimitCache) -> Result<(), FuseError> {
    if cache.schema != CACHE_SCHEMA
        || cache.models.is_empty()
        || cache.models.len() > MAX_CACHE_ENTRIES
        || cache.models.iter().any(|(key, limit)| {
            *limit == 0
                || key.split_once('/').is_none_or(|(provider, model)| {
                    provider.contains('/')
                        || model.contains('/')
                        || !is_object_name(provider)
                        || !is_object_name(model)
                })
        })
    {
        return Err(FuseError::InvalidContent);
    }
    Ok(())
}

impl FuseError {
    fn from_metadata_source(error: &MetadataSourceError) -> Self {
        match *error {
            MetadataSourceError::CacheOversize(_) => Self::TooLarge,
            MetadataSourceError::CacheMissing
            | MetadataSourceError::CacheInvalid(_, _)
            | MetadataSourceError::InvalidRemote(_)
            | MetadataSourceError::CacheCorrupt { .. } => Self::InvalidContent,
            MetadataSourceError::CacheIoRead { .. }
            | MetadataSourceError::CacheIoWrite { .. }
            | MetadataSourceError::FetchFailed(_) => Self::Io,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use cortexfs_metadatas::{MetadataCatalog, MetadataSourceError, ModelMetadata};

    use super::*;

    type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

    fn model(id: &str, context: u32) -> ModelMetadata {
        ModelMetadata::new("openai", id, id).with_context(context)
    }

    fn catalog(count: usize) -> TestResult<MetadataCatalog> {
        let mut catalog = MetadataCatalog::new();
        for index in 0..count {
            catalog.register(model(&format!("m{index}"), 1))?;
        }
        Ok(catalog)
    }

    #[test]
    fn cache_lookup_returns_known_limit() -> TestResult<()> {
        let dir = tempfile::tempdir()?;
        fs::write(
            model_limit_cache_path(dir.path()),
            r#"{"schema":"cortexfs.model-limits/v1","models":{"openai/gpt-5":272000}}"#,
        )?;

        assert_eq!(
            cached_model_limits(dir.path()).get("openai/gpt-5"),
            Some(&272_000)
        );
        Ok(())
    }

    #[test]
    fn invalid_caches_are_ignored() -> TestResult<()> {
        let dir = tempfile::tempdir()?;
        let path = model_limit_cache_path(dir.path());
        for content in [
            "not json",
            r#"{"schema":"wrong","models":{"openai/gpt-5":1}}"#,
            r#"{"schema":"cortexfs.model-limits/v1","models":{},"extra":true}"#,
        ] {
            fs::write(&path, content)?;
            assert!(cached_model_limits(dir.path()).is_empty());
        }
        fs::write(&path, vec![b'x'; usize::try_from(MAX_CACHE_BYTES)? + 1])?;
        assert!(cached_model_limits(dir.path()).is_empty());
        fs::remove_file(&path)?;
        let target = dir.path().join("target");
        fs::write(
            &target,
            r#"{"schema":"cortexfs.model-limits/v1","models":{"openai/gpt-5":1}}"#,
        )?;
        symlink(&target, &path)?;
        assert!(cached_model_limits(dir.path()).is_empty());
        Ok(())
    }

    #[test]
    fn cache_from_catalog_rejects_zero_context() -> TestResult<()> {
        let mut catalog = MetadataCatalog::new();
        catalog.register(model("gpt-no-limit", 0))?;

        assert!(matches!(
            cache_from_catalog(&catalog),
            Err(FuseError::InvalidContent)
        ));

        catalog.register(model("gpt-5", 272_000))?;
        let Ok(cache) = cache_from_catalog(&catalog) else {
            return Err("valid catalog was rejected".into());
        };
        assert_eq!(cache.models.get("openai/gpt-5"), Some(&272_000));
        assert!(!cache.models.contains_key("openai/gpt-no-limit"));
        Ok(())
    }

    #[test]
    fn cache_from_catalog_accepts_4096_entries_and_rejects_4097() -> TestResult<()> {
        let Ok(cache) = cache_from_catalog(&catalog(MAX_CACHE_ENTRIES)?) else {
            return Err("entry limit was rejected".into());
        };
        assert_eq!(cache.models.len(), MAX_CACHE_ENTRIES);
        assert!(matches!(
            cache_from_catalog(&catalog(MAX_CACHE_ENTRIES + 1)?),
            Err(FuseError::TooLarge)
        ));
        Ok(())
    }

    #[test]
    fn fetch_error_empty_and_timeout_preserve_prior_cache() -> TestResult<()> {
        let dir = tempfile::tempdir()?;
        let path = model_limit_cache_path(dir.path());
        let prior = b"prior bytes\n";
        fs::write(&path, prior)?;

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()?;
        assert!(
            runtime
                .block_on(refresh_from(dir.path(), async {
                    Err(MetadataSourceError::FetchFailed("failed".to_owned()))
                }))
                .is_err()
        );
        assert_eq!(fs::read(&path)?, prior);
        assert!(
            runtime
                .block_on(refresh_timed(
                    dir.path(),
                    Duration::ZERO,
                    std::future::pending(),
                ))
                .is_err()
        );
        assert_eq!(fs::read(&path)?, prior);
        Ok(())
    }

    #[test]
    fn refresh_rejects_symlink_parent_without_publishing() -> TestResult<()> {
        let root = tempfile::tempdir()?;
        let outside = root.path().join("outside");
        let link = root.path().join("link");
        fs::create_dir_all(&outside)?;
        symlink(&outside, &link)?;
        let catalog = catalog(1)?;
        let runtime = tokio::runtime::Builder::new_current_thread().build()?;

        assert!(
            runtime
                .block_on(refresh_from(&link.join("cache"), async { Ok(catalog) }))
                .is_err()
        );
        assert!(!outside.join("cache/model-limits.json").exists());
        Ok(())
    }
}
