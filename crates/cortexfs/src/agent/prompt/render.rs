use crate::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentPromptContext {
    pub template: String,
    pub rules: String,
    pub skills: String,
    pub tool_injection: String,
    pub history_messages: String,
    pub current_time_unix: String,
}

impl AgentPromptContext {
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            template: env::var("CTX_AGENT_PROMPT_TEMPLATE")
                .unwrap_or_else(|_error| DEFAULT_AGENT_PROMPT_TEMPLATE.to_owned()),
            rules: env::var("CTX_AGENT_RULES")
                .unwrap_or_else(|_error| "(no AGENTS.md rules injected)".to_owned()),
            skills: env::var("CTX_AGENT_SKILLS")
                .unwrap_or_else(|_error| "(no skill metadata injected)".to_owned()),
            tool_injection: env::var("CTX_AGENT_TOOL_CONTEXT")
                .unwrap_or_else(|_error| default_agent_tool_context()),
            history_messages: env::var("CTX_AGENT_HISTORY_MESSAGES")
                .unwrap_or_else(|_error| "(no historical messages injected)".to_owned()),
            current_time_unix: env::var("CTX_AGENT_CURRENT_TIME_UNIX")
                .unwrap_or_else(|_error| "0".to_owned()),
        }
    }
}

#[must_use]
pub fn default_agent_tool_context() -> String {
    "\
Runtime workspace context:
- `/workspace` is the agent's project workspace when mounted.
- `CTX_SOURCE` points at the CortexFS source view that defines visible agents, tools, models, sessions, and policy.
- `CTX_ROOT` points at the mounted CortexFS ABI root.
- Use `tsh` tools for workspace inspection and edits; do not assume file contents until you inspect them.
- No repo structure, search result, file content, or tool result has been injected yet."
        .to_owned()
}

#[must_use]
pub fn render_agent_system_prompt(
    agent: &str,
    agent_system: &str,
    prompt_context: &AgentPromptContext,
) -> String {
    let mut prompt = prompt_context.template.clone();
    let runtime_contract = agent_runtime_contract(agent);
    for (name, value) in [
        ("agent", agent),
        (
            "current_time_unix",
            prompt_context.current_time_unix.as_str(),
        ),
        ("agent_instructions", normalized_or_empty(agent_system)),
        ("rules", normalized_or_empty(&prompt_context.rules)),
        ("skills", normalized_or_empty(&prompt_context.skills)),
        (
            "tool_injection",
            normalized_or_empty(&prompt_context.tool_injection),
        ),
        (
            "history_messages",
            normalized_or_empty(&prompt_context.history_messages),
        ),
        ("runtime_contract", runtime_contract.as_str()),
    ] {
        prompt = prompt.replace(&format!("{{{{{name}}}}}"), value);
    }
    prompt
}

pub(crate) fn normalized_or_empty(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "(empty)"
    } else {
        trimmed
    }
}

#[must_use]
pub fn agent_runtime_contract(agent: &str) -> String {
    format!(
        "\
You are CortexFS agent `{agent}`.
The only native callable tool exposed by this runtime is `tsh`, the CortexFS tool shell.
Do not claim direct access to provider, host, or assistant-platform tools.
Do not mention hidden platform tools such as `image_gen` as callable tools for this agent.
Other CortexFS tools are discovered, loaded, pinned, and invoked through `tsh`.
When tool execution is useful, request the native `tsh` tool with `arguments.args` set to the exact `tsh` argv.
Tool results include the original `arguments.args` plus stdout/stderr or an ERROR line; use exact command output to decide the next repair step.
If no concrete file path is provided for a file read/write request, ask the user for a path; do not invent a project file path.
For coding work, inspect current files before editing, keep diffs small, write only files needed for the task, and run focused verification.
Never overwrite, revert, delete, or reformat unrelated user changes.
Never run destructive git commands `git reset --hard`, `git checkout --`, or `git clean` unless the user explicitly requests that exact operation.
After tool results return, continue answering the user normally.
Interactive shells and multiplexers such as bash, tmux, and zellij are ordinary CortexFS tools that must be invoked through `tsh` when visible."
    )
}
