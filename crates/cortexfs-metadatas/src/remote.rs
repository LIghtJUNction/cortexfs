use std::time::Duration;

use serde_json::Value;

use crate::MetadataSourceError;
use crate::source::MODELS_DEV_ENDPOINT;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_REMOTE_BYTES: usize = 16 * 1024 * 1024;

#[expect(
    clippy::redundant_pub_crate,
    reason = "catalog needs crate-visible fields across the private remote module"
)]
pub(crate) struct RemoteCatalog {
    pub raw: Value,
    pub observed_on: String,
}

#[expect(
    clippy::redundant_pub_crate,
    reason = "catalog refresh is called from the sibling catalog module"
)]
pub(crate) async fn fetch() -> Result<RemoteCatalog, MetadataSourceError> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| MetadataSourceError::FetchFailed(error.to_string()))?;
    let response = client
        .get(format!("{MODELS_DEV_ENDPOINT}/catalog.json"))
        .send()
        .await
        .map_err(|error| MetadataSourceError::FetchFailed(error.to_string()))?;
    if !response.status().is_success() {
        return Err(MetadataSourceError::FetchFailed(format!(
            "models.dev returned {}",
            response.status()
        )));
    }
    let observed_on = response
        .headers()
        .get(reqwest::header::DATE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| MetadataSourceError::FetchFailed(error.to_string()))?;
    if bytes.len() > MAX_REMOTE_BYTES {
        return Err(MetadataSourceError::CacheOversize(bytes.len()));
    }
    let raw: Value = serde_json::from_slice(&bytes)
        .map_err(|error| MetadataSourceError::InvalidRemote(error.to_string()))?;
    if !raw.is_object() {
        return Err(MetadataSourceError::InvalidRemote(
            "models.dev response is not an object".to_owned(),
        ));
    }
    Ok(RemoteCatalog { raw, observed_on })
}
