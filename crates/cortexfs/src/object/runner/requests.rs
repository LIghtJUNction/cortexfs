use serde_json::json;

use super::curl::run_curl_json;
use super::*;
use cortexfs::{derive_agent_runtime_view, is_object_name};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(crate) struct OpenAiProviderRequest<'a> {
    pub(crate) model: &'a str,
    pub(crate) input: &'a str,
    pub(crate) api_key: Option<&'a str>,
    pub(crate) effort: cortexfs::ModelEffort,
}
pub(crate) fn call_openai_chat(
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
pub(crate) fn call_openai_responses(
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
pub(crate) fn call_anthropic_messages(
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
pub(crate) fn openai_chat_body(
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
pub(crate) fn openai_chat_body_with_agent_tools(
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
    if stream && let Some(object) = body.as_object_mut() {
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
pub(crate) fn openai_responses_body(
    model: &str,
    input: &str,
    stream: bool,
    effort: cortexfs::ModelEffort,
) -> String {
    openai_responses_body_with_agent_tools(
        model,
        input,
        stream,
        effort,
        env::var_os("CTX_AGENT").is_some(),
    )
}
pub(crate) fn openai_responses_body_with_agent_tools(
    model: &str,
    input: &str,
    stream: bool,
    effort: cortexfs::ModelEffort,
    agent_tools: bool,
) -> String {
    let mut body = json!({
        "model": model,
        "input": provider_messages(input),
        "stream": stream
    });
    if agent_tools {
        apply_openai_responses_agent_tools(&mut body);
    }
    apply_openai_effort(&mut body, effort);
    body.to_string()
}
pub(crate) fn apply_openai_effort(body: &mut Value, effort: cortexfs::ModelEffort) {
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
pub(crate) fn apply_openai_agent_tools(body: &mut Value) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    object.insert("tools".to_owned(), Value::Array(openai_chat_tool_specs()));
    object.insert("tool_choice".to_owned(), json!("auto"));
}
pub(crate) fn apply_openai_responses_agent_tools(body: &mut Value) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    object.insert(
        "tools".to_owned(),
        Value::Array(openai_responses_tool_specs()),
    );
    object.insert("tool_choice".to_owned(), json!("auto"));
}
pub(crate) fn openai_chat_tool_specs() -> Vec<Value> {
    current_agent_openai_tools()
        .into_iter()
        .map(|name| {
            json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": openai_tool_description(&name),
                    "parameters": tsh_tool_parameters_schema()
                }
            })
        })
        .collect()
}
pub(crate) fn openai_responses_tool_specs() -> Vec<Value> {
    current_agent_openai_tools()
        .into_iter()
        .map(|name| {
            json!({
                "type": "function",
                "name": name,
                "description": openai_tool_description(&name),
                "parameters": tsh_tool_parameters_schema()
            })
        })
        .collect()
}
pub(crate) fn current_agent_openai_tools() -> Vec<String> {
    let agent = env::var("CTX_AGENT").ok();
    let root =
        env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(cortexfs::CTX_ROOT), PathBuf::from);
    current_agent_openai_tools_for(agent.as_deref(), &root)
}
pub(crate) fn current_agent_openai_tools_for(agent: Option<&str>, root: &Path) -> Vec<String> {
    let mut tools = vec!["tsh".to_owned()];
    let Some(agent) = agent else {
        return tools;
    };
    let Ok(view) = derive_agent_runtime_view(root, agent) else {
        return tools;
    };
    let state_path = cortexfs::tsh_context_state_path(view.home());
    let Ok(state) = cortexfs::read_tsh_context_state(&state_path) else {
        return tools;
    };
    for tool in state.tools {
        if provider_function_name_is_compatible(&tool.name) && !tools.contains(&tool.name) {
            tools.push(tool.name);
        }
    }
    tools.sort();
    tools.dedup();
    tools
}
pub(crate) fn provider_function_name_is_compatible(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}
pub(crate) fn openai_tool_description(name: &str) -> String {
    if name == "tsh" {
        "Invoke CortexFS tool shell. Pass exact tsh argv in args.".to_owned()
    } else {
        format!("Invoke loaded CortexFS tool `{name}` directly. Pass exact argv in args.")
    }
}
pub(crate) fn tsh_tool_parameters_schema() -> Value {
    json!({
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
    })
}
pub(crate) fn provider_messages(input: &str) -> Value {
    let agent = env::var("CTX_AGENT")
        .ok()
        .filter(|value| is_object_name(value));
    let agent_system = env::var("CTX_AGENT_SYSTEM").unwrap_or_default();
    let prompt_context = cortexfs::AgentPromptContext::from_env();
    provider_messages_for_agent(input, agent.as_deref(), &agent_system, &prompt_context)
}
pub(crate) fn provider_messages_for_agent(
    input: &str,
    agent: Option<&str>,
    agent_system: &str,
    prompt_context: &cortexfs::AgentPromptContext,
) -> Value {
    agent.map_or_else(
        || json!([{"role": "user", "content": input}]),
        |agent| {
            json!([
                {
                    "role": "system",
                    "content": cortexfs::render_agent_system_prompt(agent, agent_system, prompt_context)
                },
                {"role": "user", "content": input}
            ])
        },
    )
}
#[cfg(test)]
mod requests_tests {
    use super::*;
    fn temp_path(name: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "cortexfs-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }
    #[test]
    fn responses_agent_body_declares_tsh_function_tool() {
        let body = openai_responses_body_with_agent_tools(
            "gpt-test",
            "hello",
            true,
            cortexfs::ModelEffort::Auto,
            true,
        );
        let value = serde_json::from_str::<Value>(&body);
        assert!(value.is_ok());
        let value = value.unwrap_or_default();
        assert_eq!(value.pointer("/tools/0/type"), Some(&json!("function")));
        assert_eq!(value.pointer("/tools/0/name"), Some(&json!("tsh")));
        assert_eq!(
            value.pointer("/tools/0/parameters/properties/args/minItems"),
            Some(&json!(1))
        );
        assert_eq!(value.get("tool_choice"), Some(&json!("auto")));
    }
    #[test]
    #[expect(
        clippy::expect_used,
        reason = "test fixture setup should fail loudly with the missing field name"
    )]
    fn loaded_tools_extend_openai_tool_manifest() {
        let root = temp_path("provider-loaded-tools");
        let _ignored = fs::remove_dir_all(&root);
        let control = root.join("agent").join("coder.d");
        fs::create_dir_all(&control).expect("agent control");
        fs::write(control.join("owner"), "1000\n").expect("owner");
        fs::write(control.join("uid"), "1000\n").expect("uid");
        fs::write(control.join("gid"), "1000\n").expect("gid");
        fs::write(control.join("groups"), "1000\n").expect("groups");
        fs::write(control.join("label"), "user_u:agent_r:coder_t:s0\n").expect("label");
        fs::write(control.join("iso"), "shared\n").expect("iso");
        fs::write(control.join("parent"), "\n").expect("parent");
        fs::write(control.join("life"), "owned\n").expect("life");
        fs::write(control.join("root"), "/ctx/home/1000/agent/coder/root\n").expect("root");
        fs::write(control.join("cwd"), "/workspace\n").expect("cwd");
        fs::write(control.join("env"), "\n").expect("env");
        fs::write(
            control.join("path"),
            format!("{}\n", root.join("tool").display()),
        )
        .expect("path");
        fs::write(
            control.join("mount"),
            format!(
                "{}\t{}\tro\trbind,nosuid,nodev\n",
                root.display(),
                root.display()
            ),
        )
        .expect("mount");
        fs::write(control.join("model"), "main\n").expect("model");
        fs::write(control.join("policy"), "allow coder_t model:main use\n").expect("policy");
        fs::write(control.join("status"), "idle\n").expect("status");
        fs::write(control.join("pid"), "\n").expect("pid");
        fs::write(control.join("log"), "\n").expect("log");
        fs::write(control.join("meta.json"), "{}\n").expect("meta");
        let view = derive_agent_runtime_view(&root, "coder").expect("view");
        let mut state = cortexfs::TshContextState::default();
        state.tools = vec![
            cortexfs::TshLoadedToolState {
                name: "bash".to_owned(),
                path: root.join("tool").join("bash"),
                description: String::new(),
                schema: None,
                dynamic_resident: true,
                pinned: true,
                last_used: 1,
            },
            cortexfs::TshLoadedToolState {
                name: "shell.exec".to_owned(),
                path: root.join("tool").join("shell.exec"),
                description: String::new(),
                schema: None,
                dynamic_resident: true,
                pinned: true,
                last_used: 2,
            },
        ];
        cortexfs::write_tsh_context_state(&cortexfs::tsh_context_state_path(view.home()), &state)
            .expect("state");
        let tools = current_agent_openai_tools_for(Some("coder"), &root);
        assert_eq!(tools, vec!["bash".to_owned(), "tsh".to_owned()]);
        let _ignored = fs::remove_dir_all(root);
    }
}
