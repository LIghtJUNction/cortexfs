use std::env;
use std::time::Duration;

use reqwest::blocking::Client;
use serde_json::{Value, json};

use crate::args::EvaluateOptions;
use crate::trajectory::EvalCase;
use crate::{AppError, AppResult};

const DEFAULT_BASE_URL: &str = "https://api.futureagi.com";

pub(crate) fn evaluate(options: &EvaluateOptions, cases: &[EvalCase]) -> AppResult<Value> {
    let api_key = env::var("FI_API_KEY")
        .map_err(|_error| AppError::new("FI_API_KEY is required for cloud evaluation"))?;
    let secret_key = env::var("FI_SECRET_KEY")
        .map_err(|_error| AppError::new("FI_SECRET_KEY is required for cloud evaluation"))?;
    let environment_base_url = env::var("FI_BASE_URL").ok();
    let base_url = options
        .base_url
        .as_deref()
        .or(environment_base_url.as_deref())
        .unwrap_or(DEFAULT_BASE_URL)
        .trim_end_matches('/');
    let client = Client::builder()
        .timeout(Duration::from_secs(options.timeout))
        .build()
        .map_err(|error| AppError::new(format!("cannot create HTTP client: {error}")))?;
    let inputs = crate::registry::map_inputs(
        &client,
        base_url,
        &api_key,
        &secret_key,
        &options.eval,
        cases,
    )?;
    let payload = json!({
        "eval_name": options.eval,
        "inputs": inputs,
        "model": Value::Null,
        "span_id": Value::Null,
        "custom_eval_name": Value::Null,
        "trace_eval": false,
        "is_async": false,
        "error_localizer": false,
    });
    let response = client
        .post(format!("{base_url}/sdk/api/v1/new-eval/"))
        .header("X-Api-Key", api_key)
        .header("X-Secret-Key", secret_key)
        .json(&payload)
        .send()
        .map_err(|error| AppError::new(format!("Future AGI request failed: {error}")))?;
    let status = response.status();
    let body = crate::http::read(response, "Future AGI response")?;
    if !status.is_success() {
        return Err(AppError::new(format!(
            "Future AGI returned HTTP {status}: {}",
            String::from_utf8_lossy(&body)
        )));
    }
    serde_json::from_slice(&body)
        .map_err(|error| AppError::new(format!("Future AGI returned invalid JSON: {error}")))
}
