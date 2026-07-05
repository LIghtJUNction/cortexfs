fn parse_openai_chat_content(output: &[u8]) -> Result<String, String> {
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

fn openai_chat_tool_call_content(value: &Value) -> Option<String> {
    let function = value.get("function")?;
    let name = function.get("name").and_then(Value::as_str)?;
    if !is_object_name(name) {
        return None;
    }
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| is_object_name(id))
        .unwrap_or("call-1");
    let arguments = function.get("arguments")?;
    let args = openai_chat_tool_call_args(arguments)?;
    Some(
        json!({
            "type": "tool_call",
            "id": id,
            "name": name,
            "arguments": {
                "args": args
            }
        })
        .to_string(),
    )
}

fn openai_chat_tool_call_args(arguments: &Value) -> Option<Vec<String>> {
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

fn parse_openai_response_content(output: &[u8]) -> Result<String, String> {
    let value = serde_json::from_slice::<Value>(output)
        .map_err(|error| format!("invalid provider json: {error}"))?;
    if let Some(text) = value.get("output_text").and_then(Value::as_str)
        && !text.is_empty()
    {
        return Ok(text.to_owned());
    }
    let mut content = String::new();
    if let Some(items) = value.get("output").and_then(Value::as_array) {
        for item in items {
            let Some(parts) = item.get("content").and_then(Value::as_array) else {
                continue;
            };
            for part in parts {
                if matches!(
                    part.get("type").and_then(Value::as_str),
                    Some("output_text" | "text")
                ) && let Some(text) = part.get("text").and_then(Value::as_str)
                {
                    content.push_str(text);
                }
            }
        }
    }
    if content.is_empty() {
        Err("provider response missing content".to_owned())
    } else {
        Ok(content)
    }
}

fn parse_anthropic_message_content(output: &[u8]) -> Result<String, String> {
    let value = serde_json::from_slice::<Value>(output)
        .map_err(|error| format!("invalid provider json: {error}"))?;
    let parts = value
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| "provider response missing content".to_owned())?;
    let mut output = String::new();
    for part in parts {
        if part.get("type").and_then(Value::as_str) == Some("text")
            && let Some(text) = part.get("text").and_then(Value::as_str)
        {
            output.push_str(text);
        }
    }
    if output.is_empty() {
        Err("provider response missing text content".to_owned())
    } else {
        Ok(output)
    }
}
fn parse_provider_usage(output: &[u8]) -> Result<Option<TokenUsage>, String> {
    let value = serde_json::from_slice::<Value>(output)
        .map_err(|error| format!("invalid provider json: {error}"))?;
    Ok(token_usage_from_value(&value))
}

fn token_usage_from_value(value: &Value) -> Option<TokenUsage> {
    usage_value_candidates(value)
        .into_iter()
        .find_map(token_usage_from_usage_value)
}

fn usage_value_candidates(value: &Value) -> Vec<&Value> {
    [
        value.get("usage"),
        value.pointer("/response/usage"),
        value.pointer("/message/usage"),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn token_usage_from_usage_value(value: &Value) -> Option<TokenUsage> {
    let input_tokens = value
        .get("input_tokens")
        .or_else(|| value.get("prompt_tokens"))
        .and_then(Value::as_u64)?;
    let output_tokens = value
        .get("output_tokens")
        .or_else(|| value.get("completion_tokens"))
        .and_then(Value::as_u64)?;
    Some(TokenUsage {
        input_tokens,
        output_tokens,
    })
}

macro_rules! provider_target_fn {
    ($name:ident, $path:literal) => {
        fn $name(transport: &ResolvedTransport) -> CurlJsonTarget {
            let (base_url, unix_socket) = match *transport {
                ResolvedTransport::Direct { ref base_url }
                | ResolvedTransport::Http { ref base_url } => (base_url, None),
                ResolvedTransport::Unix {
                    ref base_url,
                    ref socket_path,
                } => (base_url, Some(socket_path.clone())),
            };
            let base = base_url.trim().trim_end_matches('/');
            let url = if base.rsplit('/').next() == Some("v1") {
                format!("{base}/{}", $path)
            } else {
                format!("{base}/v1/{}", $path)
            };
            CurlJsonTarget {
                url,
                unix_socket,
            }
        }
    };
}

provider_target_fn!(chat_completions_target, "chat/completions");
provider_target_fn!(responses_target, "responses");
provider_target_fn!(anthropic_messages_target, "messages");

fn anthropic_headers(credential: &ProviderCredential) -> Vec<String> {
    let auth = match *credential {
        ProviderCredential::Bearer(ref token) => format!("Authorization: Bearer {token}"),
        ProviderCredential::AnthropicApiKey(ref key) => format!("x-api-key: {key}"),
    };
    vec![auth, "anthropic-version: 2023-06-01".to_owned()]
}
