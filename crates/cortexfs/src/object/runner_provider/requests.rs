struct OpenAiProviderRequest<'a> {
    model: &'a str,
    input: &'a str,
    api_key: Option<&'a str>,
    effort: cortexfs::ModelEffort,
}

fn call_openai_chat(
    transport: &ResolvedTransport,
    request: &OpenAiProviderRequest<'_>,
) -> Result<ProviderTextCompletion, String> {
    let target = chat_completions_target(transport);
    let body = openai_chat_body(request.model, request.input, false, request.effort);
    let output = run_curl_json(&target, request.api_key, &body)?;
    Ok(ProviderTextCompletion {
        content: parse_openai_chat_content(&output)?,
        usage: parse_provider_usage(&output)?,
    })
}

fn call_openai_responses(
    transport: &ResolvedTransport,
    request: &OpenAiProviderRequest<'_>,
) -> Result<ProviderTextCompletion, String> {
    let target = responses_target(transport);
    let body = openai_responses_body(request.model, request.input, false, request.effort);
    let output = run_curl_json(&target, request.api_key, &body)?;
    Ok(ProviderTextCompletion {
        content: parse_openai_response_content(&output)?,
        usage: parse_provider_usage(&output)?,
    })
}

fn call_anthropic_messages(
    transport: &ResolvedTransport,
    model: &str,
    input: &str,
    credential: &ProviderCredential,
) -> Result<ProviderTextCompletion, String> {
    let target = anthropic_messages_target(transport);
    let body = json!({
        "model": model,
        "max_tokens": 4096,
        "messages": [{"role": "user", "content": input}]
    })
    .to_string();
    let headers = anthropic_headers(credential);
    let output = run_curl_json_with_headers(&target, &headers, &body)?;
    Ok(ProviderTextCompletion {
        content: parse_anthropic_message_content(&output)?,
        usage: parse_provider_usage(&output)?,
    })
}
fn openai_chat_body(
    model: &str,
    input: &str,
    stream: bool,
    effort: cortexfs::ModelEffort,
) -> String {
    openai_chat_body_with_agent_tools(
        model,
        input,
        stream,
        effort,
        env::var_os("CTX_AGENT").is_some(),
    )
}

fn openai_chat_body_with_agent_tools(
    model: &str,
    input: &str,
    stream: bool,
    effort: cortexfs::ModelEffort,
    agent_tools: bool,
) -> String {
    let mut body = json!({
        "model": model,
        "messages": provider_messages(input),
        "stream": stream
    });
    if stream
        && let Some(object) = body.as_object_mut()
    {
        object.insert(
            "stream_options".to_owned(),
            json!({
                "include_usage": true
            }),
        );
    }
    if agent_tools {
        apply_openai_agent_tools(&mut body);
    }
    apply_openai_effort(&mut body, effort);
    body.to_string()
}

fn openai_responses_body(
    model: &str,
    input: &str,
    stream: bool,
    effort: cortexfs::ModelEffort,
) -> String {
    let mut body = json!({
        "model": model,
        "input": provider_messages(input),
        "stream": stream
    });
    apply_openai_effort(&mut body, effort);
    body.to_string()
}

fn apply_openai_effort(body: &mut Value, effort: cortexfs::ModelEffort) {
    if effort == cortexfs::ModelEffort::Auto {
        return;
    }
    if let Some(object) = body.as_object_mut() {
        object.insert(
            "reasoning".to_owned(),
            json!({
                "effort": effort.to_string()
            }),
        );
    }
}

fn apply_openai_agent_tools(body: &mut Value) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    object.insert(
        "tools".to_owned(),
        json!([{
            "type": "function",
            "function": {
                "name": "tsh",
                "description": "Invoke the CortexFS tool shell. Pass exact tsh argv in args.",
                "parameters": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "args": {
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 1
                        }
                    },
                    "required": ["args"]
                }
            }
        }]),
    );
    object.insert("tool_choice".to_owned(), json!("auto"));
}
fn provider_messages(input: &str) -> Value {
    let agent = env::var("CTX_AGENT")
        .ok()
        .filter(|value| is_object_name(value));
    let agent_system = env::var("CTX_AGENT_SYSTEM").unwrap_or_default();
    let prompt_context = cortexfs::AgentPromptContext::from_env();
    provider_messages_for_agent(input, agent.as_deref(), &agent_system, &prompt_context)
}

fn provider_messages_for_agent(
    input: &str,
    agent: Option<&str>,
    agent_system: &str,
    prompt_context: &cortexfs::AgentPromptContext,
) -> Value {
    agent.map_or_else(
        || json!([{"role": "user", "content": input}]),
        |agent| json!([
            {
                "role": "system",
                "content": cortexfs::render_agent_system_prompt(agent, agent_system, prompt_context)
            },
            {"role": "user", "content": input}
        ]),
    )
}
