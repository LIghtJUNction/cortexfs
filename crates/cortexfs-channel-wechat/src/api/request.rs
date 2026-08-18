use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::Value;

use crate::{
    config::Config,
    error::{Error, Result},
};

pub(super) fn headers(config: &Config) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        "AuthorizationType",
        HeaderValue::from_static("ilink_bot_token"),
    );
    headers.insert(
        "X-WECHAT-UIN",
        HeaderValue::from_str(&config.wechat_uin)
            .map_err(|_error| Error::Protocol("generated WeChat identity is invalid".to_owned()))?,
    );
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", config.token))
            .map_err(|_error| Error::Config("CORTEXFS_WECHAT_TOKEN is invalid".to_owned()))?,
    );
    Ok(headers)
}

pub(super) fn check(value: &Value) -> Result<()> {
    let ret = value.get("ret").and_then(Value::as_i64).unwrap_or(0);
    let code = value.get("errcode").and_then(Value::as_i64).unwrap_or(0);
    if ret != 0 || code != 0 {
        let reason = value
            .get("errmsg")
            .and_then(Value::as_str)
            .unwrap_or("request rejected");
        return Err(Error::Api(reason.to_owned()));
    }
    Ok(())
}

pub(super) fn client(config: &Config) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(config.poll_timeout + Duration::from_secs(10))
        .build()?)
}

pub(super) fn client_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |value| value.as_millis());
    format!("cortexfs-{millis}")
}
