use reqwest::blocking::Client;
use serde_json::{Map, Value};

use crate::trajectory::EvalCase;
use crate::{AppError, AppResult};

pub(crate) fn map_inputs(
    client: &Client,
    base_url: &str,
    api_key: &str,
    secret_key: &str,
    eval_name: &str,
    cases: &[EvalCase],
) -> AppResult<Map<String, Value>> {
    let case = cases
        .first()
        .ok_or_else(|| AppError::new("trajectory produced no evaluation case"))?;
    let serialized = serde_json::to_value(case)
        .map_err(|error| AppError::new(format!("cannot encode evaluation input: {error}")))?;
    let input = serialized
        .as_object()
        .cloned()
        .ok_or_else(|| AppError::new("evaluation input is not a JSON object"))?;
    let selected = match required_keys(client, base_url, api_key, secret_key, eval_name) {
        Some(required) if !required.is_empty() => {
            let selected = select_keys(&input, &required);
            if let Some(missing) = required.iter().find(|key| !selected.contains_key(*key)) {
                return Err(AppError::new(format!(
                    "trajectory cannot provide `{missing}` required by evaluator `{eval_name}`"
                )));
            }
            selected
        }
        _ => input,
    };
    Ok(selected
        .into_iter()
        .map(|(key, value)| (key, Value::Array(vec![value])))
        .collect())
}

fn required_keys(
    client: &Client,
    base_url: &str,
    api_key: &str,
    secret_key: &str,
    eval_name: &str,
) -> Option<Vec<String>> {
    let response = client
        .get(format!("{base_url}/sdk/api/v1/get-evals/"))
        .header("X-Api-Key", api_key)
        .header("X-Secret-Key", secret_key)
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body = crate::http::read(response, "Future AGI evaluator registry").ok()?;
    let body = serde_json::from_slice::<Value>(&body).ok()?;
    body.get("result")?
        .as_array()?
        .iter()
        .find(|item| item.get("name").and_then(Value::as_str) == Some(eval_name))
        .and_then(|item| item.pointer("/config/required_keys"))
        .and_then(Value::as_array)
        .map(|keys| {
            keys.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
}

fn select_keys(input: &Map<String, Value>, required: &[String]) -> Map<String, Value> {
    required
        .iter()
        .filter_map(|key| {
            input
                .get(key)
                .or_else(|| aliases(key).iter().find_map(|alias| input.get(*alias)))
                .map(|value| (key.clone(), value.clone()))
        })
        .collect()
}

fn aliases(key: &str) -> &'static [&'static str] {
    match key {
        "output" => &["response", "answer", "generated", "input"],
        "response" | "answer" | "generated" | "text" | "content" => &["output"],
        "input" => &["query", "question", "prompt_input", "output"],
        "query" | "question" | "prompt_input" | "prompt" => &["input"],
        "contexts" => &["context"],
        "context" => &["contexts"],
        "expected" | "expected_value" | "reference" => {
            &["expected_output", "expected_response", "ground_truth"]
        }
        "conversation" => &["messages"],
        "system_prompt" => &["instructions", "prompt"],
        _ => &[],
    }
}
