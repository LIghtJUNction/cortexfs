use serde_json::json;

use super::*;
use cortexfs_protocol::{EventStatus, ModelEvent, ToolCall, WireProtocol, decode_response_events};
use serde_json::Value;

use crate::provider::openai_response_item_requires_continuation;
use cortexfs::is_object_name;

pub(crate) fn parse_provider_content(
    protocol: WireProtocol,
    output: &[u8],
) -> Result<String, String> {
    let value = serde_json::from_slice::<Value>(output)
        .map_err(|error| format!("invalid provider json: {error}"))?;
    validate_provider_response(protocol, &value)?;
    let events = decode_response_events(protocol, output).map_err(|error| error.to_string())?;
    normalized_content(&events)
}

#[cfg(test)]
pub(crate) fn parse_openai_chat_content(output: &[u8]) -> Result<String, String> {
    parse_provider_content(WireProtocol::OpenAiChat, output)
}
pub(crate) fn openai_chat_finish_reason(value: &Value) -> Result<Option<&str>, String> {
    let reason = value
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
        .filter(|reason| !reason.trim().is_empty());
    if let Some(reason @ ("length" | "content_filter")) = reason {
        return Err(format!("provider response finished with {reason}"));
    }
    Ok(reason)
}
pub(crate) fn openai_chat_tool_call_content(value: &Value) -> Option<String> {
    let function = value.get("function")?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| is_object_name(id))?;
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
#[cfg(test)]
pub(crate) fn parse_openai_response_content(output: &[u8]) -> Result<String, String> {
    parse_provider_content(WireProtocol::OpenAiResponses, output)
}

#[cfg(test)]
pub(crate) fn parse_anthropic_message_content(output: &[u8]) -> Result<String, String> {
    parse_provider_content(WireProtocol::Anthropic, output)
}

fn validate_provider_response(protocol: WireProtocol, value: &Value) -> Result<(), String> {
    if protocol == WireProtocol::OpenAiChat {
        openai_chat_finish_reason(value)?;
    }
    if protocol == WireProtocol::Gemini {
        return gemini_response_status(value);
    }
    if protocol != WireProtocol::OpenAiResponses {
        return Ok(());
    }
    if let Some((path, status)) = match value.get("status").and_then(Value::as_str) {
        Some(status @ ("failed" | "cancelled")) => Some(("/error/message", status)),
        Some("incomplete") => Some(("/incomplete_details/reason", "incomplete")),
        _ => None,
    } {
        return Err(value
            .pointer(path)
            .and_then(Value::as_str)
            .map_or_else(|| format!("provider response {status}"), str::to_owned));
    }
    let items: &[Value] = value
        .get("output")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    if items.iter().any(openai_response_item_requires_continuation) {
        return Err("provider response requires host-owned program continuation".to_owned());
    }
    Ok(())
}

/// Rejects a Gemini `generateContent` body that carries no usable candidate.
///
/// The Gemini decoder only maps `SAFETY` to an error status, so the runner
/// mirrors the `openai.chat` path and refuses truncated or filtered answers
/// before they reach the session recorder as ordinary text.
fn gemini_response_status(value: &Value) -> Result<(), String> {
    if let Some(message) = value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .filter(|message| !message.trim().is_empty())
    {
        return Err(format!("provider response failed: {message}"));
    }
    if let Some(reason) = value
        .pointer("/promptFeedback/blockReason")
        .and_then(Value::as_str)
    {
        return Err(format!("provider response blocked with {reason}"));
    }
    match value
        .pointer("/candidates/0/finishReason")
        .and_then(Value::as_str)
    {
        Some("STOP") | None => Ok(()),
        Some(reason) => Err(format!("provider response finished with {reason}")),
    }
}

fn normalized_content(events: &[ModelEvent]) -> Result<String, String> {
    if let Some(call) = events.iter().find_map(tool_call_content) {
        return Ok(call);
    }
    let text = events
        .iter()
        .filter_map(|event| match *event {
            ModelEvent::TextDelta { ref text, .. }
            | ModelEvent::ReasoningDelta { ref text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>();
    if events.iter().any(|event| {
        matches!(
            event,
            ModelEvent::Done {
                status: EventStatus::Error | EventStatus::Cancelled,
                ..
            }
        )
    }) {
        return Err("provider response failed".to_owned());
    }
    if text.is_empty() {
        Err("provider response missing content".to_owned())
    } else {
        Ok(text)
    }
}

fn tool_call_content(event: &ModelEvent) -> Option<String> {
    let ModelEvent::ToolCall { ref call, .. } = *event else {
        return None;
    };
    canonical_event_tool_call(call)
}

fn canonical_event_tool_call(call: &ToolCall) -> Option<String> {
    if !provider_function_name_is_compatible(&call.name) || !is_object_name(&call.id) {
        return None;
    }
    let args = openai_chat_tool_call_args(&call.arguments)?;
    Some(
        json!({"type":"tool_call","id":call.id,"name":call.name,"arguments":{"args":args}})
            .to_string(),
    )
}
pub(crate) fn openai_response_tool_call_content(value: &Value) -> Option<String> {
    if value.get("type").and_then(Value::as_str) != Some("function_call") {
        return None;
    }
    let id = value
        .get("call_id")
        .and_then(Value::as_str)
        .filter(|id| is_object_name(id))?;
    canonical_tool_call(value.get("name")?.as_str()?, id, value.get("arguments")?)
}
fn canonical_tool_call(name: &str, id: &str, arguments: &Value) -> Option<String> {
    if !provider_function_name_is_compatible(name) {
        return None;
    }
    let args = openai_chat_tool_call_args(arguments)?;
    Some(json!({"type":"tool_call","id":id,"name":name,"arguments":{"args":args}}).to_string())
}
pub(crate) fn text_parts<'a>(parts: impl Iterator<Item = &'a Value>) -> String {
    parts
        .filter_map(|part| match part.get("type").and_then(Value::as_str) {
            Some("output_text" | "text") => part.get("text").and_then(Value::as_str),
            Some("refusal") => part.get("refusal").and_then(Value::as_str),
            _ => None,
        })
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
        value.get("usageMetadata"),
        value.pointer("/response/usage"),
        value.pointer("/response/usageMetadata"),
        value.pointer("/message/usage"),
    ]
    .into_iter()
    .flatten()
    .find_map(|value| {
        Some(TokenUsage {
            input_tokens: usage_count(
                value,
                &["/input_tokens", "/prompt_tokens", "/promptTokenCount"],
            )?,
            output_tokens: usage_count(
                value,
                &[
                    "/output_tokens",
                    "/completion_tokens",
                    "/candidatesTokenCount",
                ],
            )?,
            cached_tokens: usage_count(
                value,
                &[
                    "/input_tokens_details/cached_tokens",
                    "/prompt_tokens_details/cached_tokens",
                    "/cachedContentTokenCount",
                ],
            ),
            cache_write_tokens: usage_count(
                value,
                &[
                    "/input_tokens_details/cache_write_tokens",
                    "/prompt_tokens_details/cache_write_tokens",
                ],
            ),
        })
    })
}
/// Reads the first present token count among provider-specific spellings.
fn usage_count(value: &Value, pointers: &[&str]) -> Option<u64> {
    pointers
        .iter()
        .filter_map(|pointer| value.pointer(pointer))
        .find_map(Value::as_u64)
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
pub(crate) fn provider_request_target(
    transport: &ResolvedTransport,
    credential: Option<&ProviderCredential>,
    protocol: WireProtocol,
    model: &str,
    run: &str,
) -> Result<(CurlJsonTarget, Vec<String>), String> {
    match protocol {
        WireProtocol::OpenAiChat => openai_target(transport, credential, false, run),
        WireProtocol::OpenAiResponses => openai_target(transport, credential, true, run),
        WireProtocol::Anthropic => {
            let credential = credential.ok_or_else(|| "missing Anthropic credential".to_owned())?;
            Ok((
                provider_target(transport, "messages"),
                anthropic_headers(credential)?,
            ))
        }
        WireProtocol::Gemini => {
            let credential = credential.ok_or_else(|| "missing Gemini credential".to_owned())?;
            Ok((
                gemini_target(transport, model)?,
                gemini_headers(credential)?,
            ))
        }
    }
}

/// Builds the Gemini `models/<model>:generateContent` target.
///
/// Gemini binds the API version into the provider base URL and the model into
/// the request path, so the shared `/v1` normalization must not apply here.
fn gemini_target(transport: &ResolvedTransport, model: &str) -> Result<CurlJsonTarget, String> {
    let model = model.strip_prefix("models/").unwrap_or(model);
    if !is_object_name(model) {
        return Err("invalid Gemini model name".to_owned());
    }
    let (base_url, unix_socket) = match *transport {
        ResolvedTransport::Direct { ref base_url } | ResolvedTransport::Http { ref base_url } => {
            (base_url, None)
        }
        ResolvedTransport::Unix {
            ref base_url,
            ref socket_path,
        } => (base_url, Some(socket_path.clone())),
    };
    Ok(CurlJsonTarget {
        url: format!(
            "{}/models/{model}:generateContent",
            base_url.trim().trim_end_matches('/')
        ),
        unix_socket,
    })
}

/// Selects the Gemini authentication header for one resolved credential.
///
/// Google splits authentication by credential kind: API keys travel in
/// `x-goog-api-key` while OAuth access tokens use the bearer header.
fn gemini_headers(credential: &ProviderCredential) -> Result<Vec<String>, String> {
    match *credential {
        ProviderCredential::GoogleApiKey(ref key) => Ok(vec![format!("x-goog-api-key: {key}")]),
        ProviderCredential::Bearer(ref token) => Ok(vec![format!("Authorization: Bearer {token}")]),
        ProviderCredential::AnthropicApiKey(_) | ProviderCredential::Codex { .. } => {
            Err("invalid Gemini credential".to_owned())
        }
    }
}

#[cfg(test)]
pub(crate) fn openai_request_target(
    transport: &ResolvedTransport,
    credential: Option<&ProviderCredential>,
    responses: bool,
    run: &str,
) -> Result<(CurlJsonTarget, Vec<String>), String> {
    openai_target(transport, credential, responses, run)
}

fn openai_target(
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
pub(crate) fn anthropic_headers(credential: &ProviderCredential) -> Result<Vec<String>, String> {
    let auth = match *credential {
        ProviderCredential::Bearer(ref token) | ProviderCredential::Codex { ref token, .. } => {
            format!("Authorization: Bearer {token}")
        }
        ProviderCredential::AnthropicApiKey(ref key) => format!("x-api-key: {key}"),
        ProviderCredential::GoogleApiKey(_) => {
            return Err("invalid Anthropic credential".to_owned());
        }
    };
    Ok(vec![auth, "anthropic-version: 2023-06-01".to_owned()])
}
