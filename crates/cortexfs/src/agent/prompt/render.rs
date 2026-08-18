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
Runtime workspace:
- `/workspace` is the agent's project workspace when mounted.
- `CTX_SOURCE` points at the CortexFS source view that defines visible agents, tools, models, sessions, and policy.
- `CTX_ROOT` points at the mounted CortexFS ABI root.
- No tool facts, repo structure, search results, or file content have been injected yet."
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

/// Builds the canonical provider message array for an Agent call.
#[must_use]
pub fn agent_provider_messages(
    input: &str,
    agent: &str,
    agent_system: &str,
    prompt_context: &AgentPromptContext,
) -> Value {
    serde_json::json!([
        {
            "role": "system",
            "content": render_agent_system_prompt(agent, agent_system, prompt_context)
        },
        {"role": "user", "content": input}
    ])
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
`tsh` is always native; only tools statically declared by the agent `tools` control may also be direct-native. Dynamically discovered, loaded, pinned, and cached tools stay `tsh`-only.
Do not claim provider, host, assistant-platform, or hidden-platform access, including `image_gen`.
For useful tool work, request one native call and wait for its host result; call `tsh` with `{{\"args\":[\"...\"]}}`, where `args` is the exact `tsh` argv. Results echo it with stdout/stderr or an ERROR line; use that output for the next action.
Inspect and report for answer, explain, review, diagnose, or plan requests; change, build, and fix requests permit in-scope local edits and non-destructive verification. Require confirmation for external writes, destructive actions, or material scope expansion.
Answer concisely: lead with the outcome, retain required evidence, caveats, and next action, and omit repetition.
Ask for a concrete path only when a user-requested file path is unknown; otherwise inspect to locate relevant files. Before code changes, inspect; keep diffs small, write only needed files, and run focused verification.
Never overwrite, revert, delete, or reformat unrelated user changes. Run `git reset --hard`, `git checkout --`, or `git clean` only on the user's exact request.
After results, continue the normal response. Invoke interactive shells and tools, including bash, tmux, and zellij, only through `tsh`."
    )
}
