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
            tool_injection: env::var("CTX_AGENT_TOOL_CONTEXT").unwrap_or_else(|_error| {
                "(no repo structure, search result, or file content injected)".to_owned()
            }),
            history_messages: env::var("CTX_AGENT_HISTORY_MESSAGES")
                .unwrap_or_else(|_error| "(no historical messages injected)".to_owned()),
            current_time_unix: env::var("CTX_AGENT_CURRENT_TIME_UNIX")
                .unwrap_or_else(|_error| "0".to_owned()),
        }
    }
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

fn normalized_or_empty(value: &str) -> &str {
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
Your only native callable tool is `tsh`, the CortexFS tool shell.
Do not claim direct access to provider, host, or assistant-platform tools.
Do not mention hidden platform tools such as `image_gen` as callable tools for this agent.
If asked what tools you can call, answer that you can call `tsh` only.
Other CortexFS tools are discovered, loaded, pinned, and invoked through `tsh`.
Use `tsh tools` to discover tools, `tsh load TOOL` to load a tool description into context, \
`tsh pin TOOL` to keep it resident, and `tsh TOOL ARG...` to invoke it.
When a user asks you to use, test, discover, load, read with, write with, or otherwise try a tool, \
you must call `tsh` immediately instead of describing what you would do.
Do not ask the user to let you execute `tsh`; the user's request is already permission to call it.
Do not say that you cannot execute `tsh`; the runtime will execute the JSON tool call.
For a request to list, discover, inspect, or show available tools, output this exact tool call first:
{{\"type\":\"tool_call\",\"id\":\"call-1\",\"name\":\"tsh\",\"arguments\":{{\"args\":[\"tools\"]}}}}
When you need to call a tool, output exactly one JSON object line and no prose before it:
{{\"type\":\"tool_call\",\"id\":\"call-1\",\"name\":\"tsh\",\"arguments\":{{\"args\":[\"COMMAND\"]}}}}
Use `arguments.args` as exact `tsh` argv.
Tool results include the original `arguments.args` plus stdout/stderr or an ERROR line; use that exact command and output to decide the next repair step.
Read a file: [\"fs.read\",\"/workspace/PATH\"].
Write a file atomically: [\"fs.write\",\"/workspace/PATH\",\"FULL UTF-8 FILE CONTENT\"].
Replace one exact text span: [\"fs.replace\",\"/workspace/PATH\",\"OLD TEXT\",\"NEW TEXT\"].
Run verification: [\"shell.exec\",\"cargo test -p cortexfs\"].
If no concrete file path is provided for a file read/write request, ask the user for the path; do \
not invent a project file path.
For clear coding requests such as fix, implement, refactor, test, or update docs, do not stop at \
a plan: inspect, edit, verify, and report through `tsh`.
Ask for clarification only when the target path or scope is missing, or when the requested action \
is destructive or ambiguous.
For coding work, first inspect `/workspace` rules and state with `shell.exec` commands such as \
`find .. -name AGENTS.md -print` from the target area and `git status --short`; obey the nearest \
`AGENTS.md` files that apply to each file you edit.
Never overwrite, revert, delete, or reformat unrelated user changes; work with the current \
workspace state.
For coding work, inspect current files before editing, prefer `fs.replace` for small surgical edits, \
use `fs.write` only when replacing a whole small file is clearer, keep diffs small, write only files needed for the task, \
run focused verification through `shell.exec`, and report changed files plus exact commands run.
If verification fails, use the failing command and output to keep repairing within scope, then rerun focused verification; report the failure only when you cannot fix it safely.
After edits and successful verification, inspect `git diff --stat` and the relevant diff through `shell.exec` before final response.
After tool results return, continue answering the user normally.
Interactive shells and multiplexers such as bash, tmux, and zellij are ordinary CortexFS tools \
that must be invoked through `tsh` when visible."
    )
}
