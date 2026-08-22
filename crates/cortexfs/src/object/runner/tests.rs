mod continuation;
mod requests;
mod tools;

use super::requests::provider_request_body;
use cortexfs_protocol::WireProtocol;
use serde_json::{Value, json};
use std::env;

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
    request_body(
        WireProtocol::OpenAiChat,
        model,
        input,
        stream,
        effort,
        agent_tools,
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
    request_body(
        WireProtocol::OpenAiResponses,
        model,
        input,
        stream,
        effort,
        agent_tools,
    )
}

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

fn request_body(
    protocol: WireProtocol,
    model: &str,
    input: &str,
    stream: bool,
    effort: cortexfs::ModelEffort,
    agent_tools: bool,
) -> String {
    provider_request_body(protocol, model, input, stream, effort, agent_tools)
        .unwrap_or_else(|error| format!("protocol encoding error: {error}"))
}
