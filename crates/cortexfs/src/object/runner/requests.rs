use serde_json::json;

use super::curl::run_curl_json_with_headers;
use super::*;
use cortexfs::{derive_agent_runtime_view, is_object_name};
use cortexfs_protocol::{
    Message, ModelRequest, ToolChoice, ToolDefinition, WireProtocol, encode_model_request,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(crate) struct ProviderRequest<'a> {
    pub(crate) model: &'a str,
    pub(crate) input: &'a str,
    pub(crate) credential: Option<&'a ProviderCredential>,
    pub(crate) effort: cortexfs::ModelEffort,
}
pub(crate) fn call_provider(
    transport: &ResolvedTransport,
    protocol: WireProtocol,
    request: &ProviderRequest<'_>,
    run: &str,
) -> Result<ProviderTextCompletion, String> {
    let (target, headers) =
        provider_request_target(transport, request.credential, protocol, request.model, run)?;
    let agent_tools = protocol != WireProtocol::Anthropic && env::var_os("CTX_AGENT").is_some();
    let body = provider_request_body(
        protocol,
        request.model,
        request.input,
        false,
        request.effort,
        agent_tools,
    )?;
    let output = run_curl_json_with_headers(&target, &headers, &body)?;
    Ok(ProviderTextCompletion {
        content: parse_provider_content(protocol, &output)?,
        usage: parse_provider_usage(&output)?,
    })
}
pub(crate) fn provider_request_body(
    protocol: WireProtocol,
    model: &str,
    input: &str,
    stream: bool,
    effort: cortexfs::ModelEffort,
    agent_tools: bool,
) -> Result<String, String> {
    let mut request = model_request(protocol, model, input, stream, effort, agent_tools);
    if stream && protocol == WireProtocol::OpenAiChat {
        request.option("stream_options", json!({ "include_usage": true }));
    }
    if !request.tools.is_empty()
        && matches!(
            protocol,
            WireProtocol::OpenAiChat | WireProtocol::OpenAiResponses
        )
    {
        request.option("parallel_tool_calls", json!(false));
    }
    let bytes = encode_model_request(protocol, &request).map_err(|error| error.to_string())?;
    let body = String::from_utf8(bytes)
        .map_err(|_error| "protocol encoder returned invalid UTF-8".to_owned())?;
    if protocol == WireProtocol::Gemini {
        return gemini_body_without_model(&body);
    }
    Ok(body)
}

/// Drops the path-bound `model` field from an encoded Gemini request body.
///
/// The neutral request keeps `model` so bodies round trip between dialects, but
/// Gemini binds the model to the request URL and rejects it in the body.
fn gemini_body_without_model(body: &str) -> Result<String, String> {
    let mut value = serde_json::from_str::<Value>(body)
        .map_err(|_error| "protocol encoder returned invalid JSON".to_owned())?;
    let Some(root) = value.as_object_mut() else {
        return Err("protocol encoder returned a non-object body".to_owned());
    };
    root.remove("model");
    serde_json::to_string(&value).map_err(|_error| "cannot encode Gemini request".to_owned())
}

fn model_request(
    protocol: WireProtocol,
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
    if matches!(
        protocol,
        WireProtocol::OpenAiChat | WireProtocol::OpenAiResponses
    ) && let Some(continuation) = agent_continuation_messages(&prompt_context.tool_injection)
    {
        request.messages.extend(continuation);
    }
    request.stream = stream;
    if agent_tools {
        request.tools = current_agent_openai_tools()
            .into_iter()
            .map(protocol_tool)
            .collect();
        request.tool_choice = Some(ToolChoice::Auto);
    }
    if effort != cortexfs::ModelEffort::Auto {
        let value = json!(effort.to_string());
        match protocol {
            WireProtocol::OpenAiChat => request.option("reasoning_effort", value),
            WireProtocol::OpenAiResponses => {
                request.option("reasoning", json!({ "effort": value }));
            }
            WireProtocol::Gemini | WireProtocol::Anthropic => {}
        }
    }
    request
}

pub(crate) fn agent_continuation_messages(context: &str) -> Option<[Message; 2]> {
    context
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix(crate::agent::TOOL_CONTINUATION_CONTEXT_PREFIX))
        .and_then(|value| serde_json::from_str(value).ok())
}

fn protocol_tool(name: String) -> ToolDefinition {
    let description = openai_tool_description(&name);
    ToolDefinition {
        name,
        description: Some(description),
        parameters: tsh_tool_parameters_schema(),
    }
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
