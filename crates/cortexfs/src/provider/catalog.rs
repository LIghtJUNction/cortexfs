use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use models_dev::ModelsDevResponse;
use serde::{Deserialize, Serialize};

use crate::support::plain::{create_plain_dir, read_small_text_file};
use crate::{AtomicReplaceOutcome, FuseV1Error, atomic_replace_text_outcome, is_object_name};

const CACHE_FILE: &str = "model-limits.json";
const CACHE_SCHEMA: &str = "cortexfs.model-limits/v1";
const MAX_CACHE_BYTES: u64 = 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 4096;
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(35);
const CATALOG_URL: &str = "https://models.dev/api.json";

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

pub(crate) fn refresh_model_limit_cache(cache_dir: &Path) -> Result<(), FuseV1Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_error| FuseV1Error::Io)?;
    runtime.block_on(refresh_timed(cache_dir, FETCH_TIMEOUT, fetch_catalog()))
}

async fn refresh_timed<F>(cache_dir: &Path, timeout: Duration, fetch: F) -> Result<(), FuseV1Error>
where
    F: Future<Output = Result<Vec<u8>, FuseV1Error>>,
{
    let body = tokio::time::timeout(timeout, fetch)
        .await
        .map_err(|_error| FuseV1Error::Io)??;
    publish_cache(cache_dir, &body)
}

#[cfg(test)]
async fn refresh_from<F>(cache_dir: &Path, fetch: F) -> Result<(), FuseV1Error>
where
    F: Future<Output = Result<Vec<u8>, FuseV1Error>>,
{
    let bytes = fetch.await?;
    publish_cache(cache_dir, &bytes)
}

fn publish_cache(cache_dir: &Path, bytes: &[u8]) -> Result<(), FuseV1Error> {
    let response = serde_json::from_slice::<ModelsDevResponse>(bytes)
        .map_err(|_error| FuseV1Error::InvalidContent)?;
    let cache = cache_from_response(response)?;
    let content = serde_json::to_string(&cache).map_err(|_error| FuseV1Error::Io)? + "\n";
    if u64::try_from(content.len()).map_err(|_error| FuseV1Error::TooLarge)? > MAX_CACHE_BYTES {
        return Err(FuseV1Error::TooLarge);
    }
    create_plain_dir(cache_dir).map_err(|_error| FuseV1Error::Io)?;
    match atomic_replace_text_outcome(&model_limit_cache_path(cache_dir), &content) {
        Ok(AtomicReplaceOutcome::Synced | AtomicReplaceOutcome::PublishedUnsynced(_)) => Ok(()),
        Err(_error) => Err(FuseV1Error::Io),
    }
}

async fn fetch_catalog() -> Result<Vec<u8>, FuseV1Error> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|_error| FuseV1Error::Io)?;
    let mut response = client
        .get(CATALOG_URL)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_error| FuseV1Error::Io)?;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_error| FuseV1Error::Io)? {
        append_chunk(&mut body, &chunk, MAX_RESPONSE_BYTES)?;
    }
    Ok(body)
}

fn append_chunk(body: &mut Vec<u8>, chunk: &[u8], limit: usize) -> Result<(), FuseV1Error> {
    if body.len() > limit || chunk.len() > limit.saturating_sub(body.len()) {
        return Err(FuseV1Error::TooLarge);
    }
    body.extend_from_slice(chunk);
    Ok(())
}

fn cache_from_response(response: ModelsDevResponse) -> Result<ModelLimitCache, FuseV1Error> {
    let mut models = BTreeMap::new();
    for (provider_key, provider) in response.providers {
        if !is_object_name(&provider_key) {
            return Err(FuseV1Error::InvalidContent);
        }
        for (model_key, model) in provider.models {
            if !is_object_name(&model_key) || model.limit.context == 0 {
                continue;
            }
            if models.len() >= MAX_CACHE_ENTRIES {
                return Err(FuseV1Error::TooLarge);
            }
            models.insert(format!("{provider_key}/{model_key}"), model.limit.context);
        }
    }
    let cache = ModelLimitCache {
        schema: CACHE_SCHEMA.to_owned(),
        models,
    };
    validate_cache(&cache)?;
    Ok(cache)
}

fn validate_cache(cache: &ModelLimitCache) -> Result<(), FuseV1Error> {
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
        return Err(FuseV1Error::InvalidContent);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use models_dev::ModelsDevResponse;

    use super::*;

    type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

    fn response(json: &str) -> TestResult<ModelsDevResponse> {
        Ok(serde_json::from_str(json)?)
    }

    fn one_model(context: u32) -> TestResult<ModelsDevResponse> {
        response(&format!(
            r#"{{"openai":{{"id":"ignored-provider-id","name":"OpenAI","npm":"pkg","env":[],"doc":"doc","models":{{"gpt-5":{{"id":"ignored-model-id","name":"GPT","attachment":false,"reasoning":true,"temperature":true,"tool_call":true,"modalities":{{"input":["text"],"output":["text"]}},"limit":{{"context":{context},"output":1}}}}}}}}}}"#
        ))
    }

    fn response_bytes(response: &ModelsDevResponse) -> TestResult<Vec<u8>> {
        Ok(serde_json::to_vec(response)?)
    }

    fn models(count: usize) -> TestResult<ModelsDevResponse> {
        let mut response = one_model(1)?;
        let provider = response.providers.get_mut("openai").ok_or("provider")?;
        let model = provider.models.remove("gpt-5").ok_or("model")?;
        for index in 0..count {
            provider.models.insert(format!("m{index}"), model.clone());
        }
        Ok(response)
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
    fn response_conversion_uses_exact_map_keys_and_rejects_zero() -> TestResult<()> {
        assert!(matches!(
            cache_from_response(one_model(0)?),
            Err(FuseV1Error::InvalidContent)
        ));

        let Ok(cache) = cache_from_response(one_model(272_000)?) else {
            return Err("valid response was rejected".into());
        };
        assert_eq!(cache.models.get("openai/gpt-5"), Some(&272_000));
        assert!(
            !cache
                .models
                .contains_key("ignored-provider-id/ignored-model-id")
        );
        Ok(())
    }

    #[test]
    fn response_collection_accepts_limit_and_rejects_one_more_byte() {
        let mut body = Vec::new();
        assert!(
            append_chunk(
                &mut body,
                &vec![b'x'; MAX_RESPONSE_BYTES],
                MAX_RESPONSE_BYTES
            )
            .is_ok()
        );
        assert_eq!(body.len(), MAX_RESPONSE_BYTES);
        assert_eq!(
            append_chunk(&mut body, b"x", MAX_RESPONSE_BYTES),
            Err(FuseV1Error::TooLarge)
        );
        assert_eq!(body.len(), MAX_RESPONSE_BYTES);
    }

    #[test]
    fn response_conversion_accepts_4096_entries_and_rejects_4097() -> TestResult<()> {
        let Ok(cache) = cache_from_response(models(MAX_CACHE_ENTRIES)?) else {
            return Err("entry limit was rejected".into());
        };
        assert_eq!(cache.models.len(), MAX_CACHE_ENTRIES);
        assert!(matches!(
            cache_from_response(models(MAX_CACHE_ENTRIES + 1)?),
            Err(FuseV1Error::TooLarge)
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
                .block_on(refresh_from(dir.path(), async { Err(FuseV1Error::Io) }))
                .is_err()
        );
        assert_eq!(fs::read(&path)?, prior);
        assert!(
            runtime
                .block_on(refresh_from(dir.path(), async { Ok(b"{}".to_vec()) }))
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
        let bytes = response_bytes(&one_model(1)?)?;
        let runtime = tokio::runtime::Builder::new_current_thread().build()?;

        assert!(
            runtime
                .block_on(refresh_from(&link.join("cache"), async { Ok(bytes) }))
                .is_err()
        );
        assert!(!outside.join("cache/model-limits.json").exists());
        Ok(())
    }
}
