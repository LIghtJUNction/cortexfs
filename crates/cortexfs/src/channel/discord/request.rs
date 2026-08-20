use std::{thread, time::Duration};

use reqwest::blocking::RequestBuilder;
use serde_json::Value;

use super::{DiscordError, effect};

const ATTEMPTS: usize = 3;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

pub(super) fn send(
    mut build: impl FnMut() -> Result<RequestBuilder, DiscordError>,
) -> Result<Value, DiscordError> {
    for attempt in 0..ATTEMPTS {
        let response = match build()?.send() {
            Ok(response) => response,
            Err(_error) if attempt + 1 < ATTEMPTS => {
                let step = u64::try_from(attempt + 1).unwrap_or(3);
                thread::sleep(Duration::from_millis(200 * step));
                continue;
            }
            Err(error) => return Err(DiscordError::Http(error)),
        };
        let status = response.status();
        let header_delay = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<f64>().ok());
        let bytes = crate::support::process::read_limited_bytes(
            response,
            MAX_RESPONSE_BYTES.saturating_add(1),
        );
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(DiscordError::Api);
        }
        let parsed = if bytes.is_empty() {
            Value::Null
        } else {
            match serde_json::from_slice(&bytes) {
                Ok(value) => value,
                Err(error) if status.is_success() => return Err(error.into()),
                Err(_error) => Value::Null,
            }
        };
        if retryable(status.as_u16()) && attempt + 1 < ATTEMPTS {
            let delay = header_delay
                .or_else(|| parsed.get("retry_after").and_then(Value::as_f64))
                .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
                .unwrap_or_else(|| 0.2 * f64::from(u32::try_from(attempt + 1).unwrap_or(3)))
                .min(30.0);
            thread::sleep(Duration::from_secs_f64(delay));
            continue;
        }
        if status.as_u16() == 401 {
            return Err(DiscordError::Authentication);
        }
        if !status.is_success() {
            return Err(DiscordError::Api);
        }
        return Ok(parsed);
    }
    Err(DiscordError::Api)
}

fn retryable(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
}

pub(super) fn auth(request: RequestBuilder, config: &super::DiscordConfig) -> RequestBuilder {
    effect::auth(request, config)
}
