use serde_json::json;

use super::*;
use serde_json::Value;

use cortexfs::is_object_name;

pub(crate) fn parse_openai_chat_content(output: &[u8]) -> Result<String, String> {
    let value = serde_json::from_slice::<Value>(output)
        .map_err(|error| format!("invalid provider json: {error}"))?;
    if let Some(content) = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .or_else(|| value.get("output_text").and_then(Value::as_str))
        && !content.is_empty()
    {
        return Ok(content.to_owned());
    }
    if let Some(tool_call) = value
        .pointer("/choices/0/message/tool_calls/0")
        .and_then(openai_chat_tool_call_content)
    {
        return Ok(tool_call);
    }
    Err("provider response missing content".to_owned())
}
pub(crate) fn openai_chat_tool_call_content(value: &Value) -> Option<String> {
    let function = value.get("function")?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| is_object_name(id))
        .unwrap_or("call-1");
    canonical_tool_call(
        function.get("name")?.as_str()?,
        id,
        function.get("arguments")?,
    )
}
pub(crate) fn openai_chat_tool_call_args(arguments: &Value) -> Option<Vec<String>> {
    let value = if let Some(arguments) = arguments.as_str() {
        serde_json::from_str::<Value>(arguments).ok()?
    } else {
        arguments.clone()
    };
    value
        .get("args")?
        .as_array()?
        .iter()
        .map(|value| value.as_str().map(str::to_owned))
        .collect()
}
pub(crate) fn parse_openai_response_content(output: &[u8]) -> Result<String, String> {
    let value = serde_json::from_slice::<Value>(output)
        .map_err(|error| format!("invalid provider json: {error}"))?;
    if let Some(text) = value.get("output_text").and_then(Value::as_str)
        && !text.is_empty()
    {
        return Ok(text.to_owned());
    }
    if let Some(tool_call) = value
        .get("output")
        .and_then(Value::as_array)
        .and_then(|items| items.iter().find_map(openai_response_tool_call_content))
    {
        return Ok(tool_call);
    }
    let content = text_parts(
        value
            .get("output")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|item| {
                item.get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            }),
    );
    if content.is_empty() {
        Err("provider response missing content".to_owned())
    } else {
        Ok(content)
    }
}
pub(crate) fn openai_response_tool_call_content(value: &Value) -> Option<String> {
    if value.get("type").and_then(Value::as_str) != Some("function_call") {
        return None;
    }
    let id = value
        .get("call_id")
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .filter(|id| is_object_name(id))
        .unwrap_or("call-1");
    canonical_tool_call(value.get("name")?.as_str()?, id, value.get("arguments")?)
}
fn canonical_tool_call(name: &str, id: &str, arguments: &Value) -> Option<String> {
    if !provider_function_name_is_compatible(name) {
        return None;
    }
    let args = openai_chat_tool_call_args(arguments)?;
    Some(json!({"type":"tool_call","id":id,"name":name,"arguments":{"args":args}}).to_string())
}
pub(crate) fn parse_anthropic_message_content(output: &[u8]) -> Result<String, String> {
    let value = serde_json::from_slice::<Value>(output)
        .map_err(|error| format!("invalid provider json: {error}"))?;
    let output = text_parts(
        value
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(|| "provider response missing content".to_owned())?
            .iter(),
    );
    if output.is_empty() {
        Err("provider response missing text content".to_owned())
    } else {
        Ok(output)
    }
}
fn text_parts<'a>(parts: impl Iterator<Item = &'a Value>) -> String {
    parts
        .filter(|part| {
            matches!(
                part.get("type").and_then(Value::as_str),
                Some("output_text" | "text")
            )
        })
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect()
}
pub(crate) fn parse_provider_usage(output: &[u8]) -> Result<Option<TokenUsage>, String> {
    let value = serde_json::from_slice::<Value>(output)
        .map_err(|error| format!("invalid provider json: {error}"))?;
    Ok(token_usage_from_value(&value))
}
pub(crate) fn token_usage_from_value(value: &Value) -> Option<TokenUsage> {
    [
        value.get("usage"),
        value.pointer("/response/usage"),
        value.pointer("/message/usage"),
    ]
    .into_iter()
    .flatten()
    .find_map(|value| {
        Some(TokenUsage {
            input_tokens: value
                .get("input_tokens")
                .or_else(|| value.get("prompt_tokens"))?
                .as_u64()?,
            output_tokens: value
                .get("output_tokens")
                .or_else(|| value.get("completion_tokens"))?
                .as_u64()?,
        })
    })
}
pub(crate) fn provider_target(transport: &ResolvedTransport, path: &str) -> CurlJsonTarget {
    let (base_url, unix_socket) = match *transport {
        ResolvedTransport::Direct { ref base_url } | ResolvedTransport::Http { ref base_url } => {
            (base_url, None)
        }
        ResolvedTransport::Unix {
            ref base_url,
            ref socket_path,
        } => (base_url, Some(socket_path.clone())),
    };
    let base = crate::provider::effective_base_url(base_url);
    CurlJsonTarget {
        url: format!("{base}/{path}"),
        unix_socket,
    }
}
pub(crate) fn openai_request_target(
    transport: &ResolvedTransport,
    credential: Option<&ProviderCredential>,
    responses: bool,
    run: &str,
) -> Result<(CurlJsonTarget, Vec<String>), String> {
    let codex = matches!(credential, Some(ProviderCredential::Codex { .. }));
    if codex && !responses {
        return Err("Codex OAuth only supports openai.responses".to_owned());
    }
    let target = if codex {
        let (url, unix_socket) = match *transport {
            ResolvedTransport::Direct { .. } => (
                "https://chatgpt.com/backend-api/codex/responses".to_owned(),
                None,
            ),
            ResolvedTransport::Http { ref base_url } => (
                format!("{}/responses", base_url.trim_end_matches('/')),
                None,
            ),
            ResolvedTransport::Unix {
                ref base_url,
                ref socket_path,
            } => (
                format!("{}/responses", base_url.trim_end_matches('/')),
                Some(socket_path.clone()),
            ),
        };
        CurlJsonTarget { url, unix_socket }
    } else if responses {
        provider_target(transport, "responses")
    } else {
        provider_target(transport, "chat/completions")
    };
    let mut headers = credential.map_or_else(Vec::new, |credential| {
        vec![format!("Authorization: Bearer {}", credential.secret())]
    });
    if let Some(account_id) = credential.and_then(ProviderCredential::codex_account) {
        if [run, account_id]
            .into_iter()
            .any(|value| value.is_empty() || value.bytes().any(|byte| byte.is_ascii_control()))
        {
            return Err("invalid Codex metadata".to_owned());
        }
        headers.push(format!("ChatGPT-Account-Id: {account_id}"));
        headers.extend([
            "originator: ctx".to_owned(),
            format!("User-Agent: cortexfs/{}", env!("CARGO_PKG_VERSION")),
            format!("session-id: {run}"),
        ]);
    }
    Ok((target, headers))
}
pub(crate) fn anthropic_headers(credential: &ProviderCredential) -> Vec<String> {
    let auth = match *credential {
        ProviderCredential::Bearer(ref token) | ProviderCredential::Codex { ref token, .. } => {
            format!("Authorization: Bearer {token}")
        }
        ProviderCredential::AnthropicApiKey(ref key) => format!("x-api-key: {key}"),
    };
    vec![auth, "anthropic-version: 2023-06-01".to_owned()]
}
