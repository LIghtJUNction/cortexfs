use super::{AgentPromptContext, render_agent_system_prompt};
use cortexfs_protocol::Message;

/// Builds the provider-neutral message sequence for one model invocation.
#[must_use]
pub fn agent_prompt_messages(
    input: &str,
    agent: Option<&str>,
    agent_system: &str,
    prompt_context: &AgentPromptContext,
) -> Vec<Message> {
    let Some(agent) = agent else {
        return vec![Message::user(input)];
    };
    vec![
        Message::system(render_agent_system_prompt(
            agent,
            agent_system,
            prompt_context,
        )),
        Message::user(input),
    ]
}
