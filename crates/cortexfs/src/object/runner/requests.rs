use serde_json::json;

use super::curl::run_curl_json_with_headers;
use super::*;
use cortexfs::{derive_agent_runtime_view, is_object_name};
use cortexfs_protocol::{Content, ContentPart, ModelRequest, ToolChoice, ToolDefinition};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub(crate) struct OpenAiProviderRequest<'a> {
    pub(crate) model: &'a str,
    pub(crate) input: &'a str,
    pub(crate) credential: Option<&'a ProviderCredential>,
    pub(crate) effort: cortexfs::ModelEffort,
}
pub(crate) fn call_openai_chat(
    transport: &ResolvedTransport,
    request: &OpenAiProviderRequest<'_>,
    run: &str,
) -> Result<ProviderTextCompletion, String> {
    let (target, headers) = openai_request_target(transport, request.credential, false, run)?;
    let body = openai_chat_body(request.model, request.input, false, request.effort);
    let output = run_curl_json_with_headers(&target, &headers, &body)?;
    Ok(ProviderTextCompletion {
        content: parse_openai_chat_content(&output)?,
        usage: parse_provider_usage(&output)?,
    })
}
pub(crate) fn call_openai_responses(
    transport: &ResolvedTransport,
    request: &OpenAiProviderRequest<'_>,
    run: &str,
) -> Result<ProviderTextCompletion, String> {
    let (target, headers) = openai_request_target(transport, request.credential, true, run)?;
    let body = openai_responses_body(request.model, request.input, false, request.effort);
    let output = run_curl_json_with_headers(&target, &headers, &body)?;
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
    let target = provider_target(transport, "messages");
    let request = model_request(model, input, false, cortexfs::ModelEffort::Auto, false);
    let body = anthropic_body(&request);
    let headers = anthropic_headers(credential);
    let output = run_curl_json_with_headers(&target, &headers, &body)?;
    Ok(ProviderTextCompletion {
        content: parse_anthropic_message_content(&output)?,
        usage: parse_provider_usage(&output)?,
    })
}
#[derive(Clone, Copy)]
enum OpenAiEndpoint {
    Chat,
    Responses,
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
    openai_body(
        model,
        input,
        stream,
        effort,
        agent_tools,
        OpenAiEndpoint::Chat,
    )
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
    openai_body(
        model,
        input,
        stream,
        effort,
        agent_tools,
        OpenAiEndpoint::Responses,
    )
}
fn openai_body(
    model: &str,
    input: &str,
    stream: bool,
    effort: cortexfs::ModelEffort,
    agent_tools: bool,
    endpoint: OpenAiEndpoint,
) -> String {
    let request = model_request(model, input, stream, effort, agent_tools);
    let messages = openai_messages(&request);
    let mut body = match endpoint {
        OpenAiEndpoint::Chat => {
            json!({ "model": request.model, "messages": messages, "stream": request.stream })
        }
        OpenAiEndpoint::Responses => {
            json!({ "model": request.model, "input": messages, "stream": request.stream })
        }
    };
    let Some(fields) = body.as_object_mut() else {
        return body.to_string();
    };
    if matches!(endpoint, OpenAiEndpoint::Chat) && stream {
        fields.insert(
            "stream_options".to_owned(),
            json!({ "include_usage": true }),
        );
    }
    if !request.tools.is_empty() {
        let tools = request
            .tools
            .iter()
            .map(|tool| openai_tool(tool, endpoint))
            .collect();
        fields.insert("tools".to_owned(), Value::Array(tools));
        fields.insert("tool_choice".to_owned(), json!("auto"));
        fields.insert("parallel_tool_calls".to_owned(), json!(false));
    }
    if let Some(value) = request.options.get("reasoning_effort") {
        match endpoint {
            OpenAiEndpoint::Chat => {
                fields.insert("reasoning_effort".to_owned(), value.clone());
            }
            OpenAiEndpoint::Responses => {
                fields.insert("reasoning".to_owned(), json!({ "effort": value }));
            }
        }
    }
    body.to_string()
}
fn openai_tool(tool: &ToolDefinition, endpoint: OpenAiEndpoint) -> Value {
    let mut function = Map::from_iter([
        ("name".to_owned(), json!(tool.name)),
        (
            "description".to_owned(),
            json!(tool.description.as_deref().unwrap_or_default()),
        ),
        ("parameters".to_owned(), tool.parameters.clone()),
        ("strict".to_owned(), json!(true)),
    ]);
    match endpoint {
        OpenAiEndpoint::Chat => json!({ "type": "function", "function": function }),
        OpenAiEndpoint::Responses => {
            function.insert("type".to_owned(), json!("function"));
            Value::Object(function)
        }
    }
}

fn model_request(
    model: &str,
    input: &str,
    stream: bool,
    effort: cortexfs::ModelEffort,
    agent_tools: bool,
) -> ModelRequest {
    let agent = env::var("CTX_AGENT")
        .ok()
        .filter(|value| is_object_name(value));
    let agent_system = env::var("CTX_AGENT_SYSTEM").unwrap_or_default();
    let prompt_context = cortexfs::AgentPromptContext::from_env();
    let mut request = ModelRequest::new(
        model,
        cortexfs::agent_prompt_messages(input, agent.as_deref(), &agent_system, &prompt_context),
    );
    request.stream = stream;
    if agent_tools {
        request.tools = current_agent_openai_tools()
            .into_iter()
            .map(protocol_tool)
            .collect();
        request.tool_choice = Some(ToolChoice::Auto);
    }
    if effort != cortexfs::ModelEffort::Auto {
        request.option("reasoning_effort", json!(effort.to_string()));
    }
    request
}

fn protocol_tool(name: String) -> ToolDefinition {
    let description = openai_tool_description(&name);
    ToolDefinition {
        name,
        description: Some(description),
        parameters: tsh_tool_parameters_schema(),
    }
}

fn openai_messages(request: &ModelRequest) -> Value {
    Value::Array(
        request
            .messages
            .iter()
            .map(|message| {
                json!({
                    "role": message.role.as_str(),
                    "content": openai_content(&message.content)
                })
            })
            .collect(),
    )
}

fn openai_content(content: &Content) -> Value {
    match *content {
        Content::Text(ref value) => json!(value),
        Content::Parts(ref parts) => Value::Array(parts.iter().map(openai_part).collect()),
    }
}

fn openai_part(part: &ContentPart) -> Value {
    match *part {
        ContentPart::Text { ref text } => json!({"type":"text", "text":text}),
        ContentPart::Image { ref uri, .. } => {
            json!({"type":"image_url", "image_url":{"url":uri}})
        }
        ContentPart::Audio { ref uri, .. } => {
            json!({"type":"audio", "audio_url":{"url":uri}})
        }
        ContentPart::Data {
            ref name,
            ref value,
        } => json!({"type":name, "data":value}),
    }
}

fn anthropic_body(request: &ModelRequest) -> String {
    let system = request
        .messages
        .iter()
        .filter(|message| matches!(message.role.as_str(), "system" | "developer"))
        .map(|message| message.content.text_value())
        .collect::<Vec<_>>()
        .join("\n\n");
    let messages = request
        .messages
        .iter()
        .filter(|message| !matches!(message.role.as_str(), "system" | "developer"))
        .map(
            |message| json!({"role":message.role.as_str(), "content":message.content.text_value()}),
        )
        .collect::<Vec<_>>();
    let mut body = json!({"model":request.model, "max_tokens":4096, "messages":messages});
    if !system.is_empty()
        && let Some(fields) = body.as_object_mut()
    {
        fields.insert("system".to_owned(), json!(system));
    }
    body.to_string()
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
    for tool in view.declared_tools() {
        if provider_function_name_is_compatible(tool) && !tools.contains(tool) {
            tools.push(tool.clone());
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
        "Invoke CortexFS tool shell. Pass exact tsh argv in args. The host returns a bounded UTF-8 observation with status ok or error; inspect errors before the next call.".to_owned()
    } else {
        format!(
            "Invoke declared CortexFS tool `{name}` directly. Pass exact argv in args. The host returns a bounded UTF-8 observation with status ok or error; inspect errors before the next call."
        )
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
#[cfg(test)]
pub(crate) fn provider_messages_for_agent(
    input: &str,
    agent: Option<&str>,
    agent_system: &str,
    prompt_context: &cortexfs::AgentPromptContext,
) -> Value {
    agent.map_or_else(
        || json!([{"role": "user", "content": input}]),
        |agent| cortexfs::agent_provider_messages(input, agent, agent_system, prompt_context),
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
    fn responses_agent_body_declares_tsh_function_tool() -> Result<(), Box<dyn std::error::Error>> {
        let effort = cortexfs::ModelEffort::Auto;
        for (body, function) in [
            (
                openai_responses_body_with_agent_tools("gpt-test", "hello", true, effort, true),
                "/tools/0",
            ),
            (
                openai_chat_body_with_agent_tools("gpt-test", "hello", true, effort, true),
                "/tools/0/function",
            ),
        ] {
            let value = serde_json::from_str::<Value>(&body)?;
            assert_eq!(value.pointer("/tools/0/type"), Some(&json!("function")));
            for (field, expected) in [
                ("name", json!("tsh")),
                ("parameters/properties/args/minItems", json!(1)),
                ("parameters/required", json!(["args"])),
                ("parameters/additionalProperties", json!(false)),
                ("strict", json!(true)),
            ] {
                assert_eq!(
                    value.pointer(&format!("{function}/{field}")),
                    Some(&expected)
                );
            }
            assert_eq!(value.get("tool_choice"), Some(&json!("auto")));
            assert_eq!(value.get("parallel_tool_calls"), Some(&json!(false)));
            assert_eq!(
                value.pointer(&format!("{function}/description")),
                Some(&json!(
                    "Invoke CortexFS tool shell. Pass exact tsh argv in args. The host returns a bounded UTF-8 observation with status ok or error; inspect errors before the next call."
                ))
            );
        }
        Ok(())
    }
    #[test]
    #[expect(
        clippy::expect_used,
        reason = "test fixture setup should fail loudly with the missing field name"
    )]
    fn declared_tools_extend_openai_tool_manifest_and_cache_does_not() {
        let root = temp_path("provider-loaded-tools");
        let _ignored = fs::remove_dir_all(&root);
        let control = root.join("agent").join("coder.d");
        fs::create_dir_all(&control).expect("agent control");
        fs::write(control.join("owner"), "1000\n").expect("owner");
        fs::write(control.join("uid"), "1000\n").expect("uid");
        fs::write(control.join("gid"), "1000\n").expect("gid");
        fs::write(control.join("groups"), "1000\n").expect("groups");
        fs::write(control.join("perm"), "rwx\n").expect("perm");
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
        fs::write(control.join("model"), "local/chat\n").expect("model");
        fs::write(control.join("abi"), "sdk-envelope-v1\n").expect("abi");
        fs::write(control.join("window"), "auto\n").expect("window");
        let model_control = root.join("model/local/chat.d");
        fs::create_dir_all(&model_control).expect("model control");
        fs::write(model_control.join("limit"), "unknown\n").expect("limit");
        fs::write(
            control.join("policy"),
            "allow coder_t model:local/chat use\n",
        )
        .expect("policy");
        fs::write(control.join("tools"), "bash\nshell.exec\n").expect("tools");
        fs::write(control.join("status"), "idle\n").expect("status");
        fs::write(control.join("pid"), "\n").expect("pid");
        fs::write(control.join("log"), "\n").expect("log");
        fs::write(control.join("meta.json"), "{}\n").expect("meta");
        let view = derive_agent_runtime_view(&root, "coder").expect("view");
        let mut state = cortexfs::TshContextState::default();
        state.tools = vec![
            cortexfs::TshLoadedToolState {
                name: "cached_only".to_owned(),
                path: root.join("tool").join("cached_only"),
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
        cortexfs::write_tsh_context_state(
            &cortexfs::tsh_context_state_path(&view.home().join("session").join("session-a")),
            &state,
        )
        .expect("state");
        let tools = current_agent_openai_tools_for(Some("coder"), &root);
        assert_eq!(tools, vec!["bash".to_owned(), "tsh".to_owned()]);
        let _ignored = fs::remove_dir_all(root);
    }
}
