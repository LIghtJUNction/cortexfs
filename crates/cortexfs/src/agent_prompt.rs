use std::collections::VecDeque;
use std::env;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::DEFAULT_AGENT_PROMPT_TEMPLATE;
use serde_json::Value;

pub const MAX_SKILL_METADATA_CHARS: usize = 8_000;
pub const MAX_HISTORY_MESSAGES_CHARS: usize = 8_000;
const MAX_AGENT_RULES_CHARS: usize = 64_000;
const MAX_AGENT_RULE_FILE_BYTES: u64 = 64 * 1024;
const MAX_SKILL_FILE_BYTES: u64 = 16 * 1024;
const MAX_SKILL_FILES: usize = 256;
const MAX_HISTORY_MESSAGES_READ_BYTES: u64 = 64 * 1024;
const MAX_HISTORY_MESSAGE_LINE_BYTES: usize = 16 * 1024;

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
If asked what tools you can call, answer that you can call `tsh` only.
Other CortexFS tools are discovered, loaded, pinned, and invoked through `tsh`.
Use `tsh tools` to discover tools, `tsh load TOOL` to load a tool description into context, \
`tsh pin TOOL` to keep it resident, and `tsh TOOL ARG...` to invoke it.
When a user asks you to use, test, discover, load, read with, write with, or otherwise try a tool, \
you must call `tsh` immediately instead of describing what you would do.
When you need to call a tool, output exactly one JSON object line and no prose before it:
{{\"type\":\"tool_call\",\"id\":\"call-1\",\"name\":\"tsh\",\"arguments\":{{\"args\":[\"tools\"]}}}}
Use `arguments.args` for the `tsh` argv, for example [\"fs.read\",\"PATH_FROM_USER\"].
If no concrete file path is provided for a file read/write request, ask the user for the path; do \
not invent a project file path.
After the tool result is returned, continue answering the user normally.
Interactive shells and multiplexers such as bash, tmux, and zellij are ordinary CortexFS tools \
that must be invoked through `tsh` when visible."
    )
}

#[must_use]
pub fn collect_agent_rules() -> String {
    let mut paths = Vec::new();
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        paths.push(home.join(".codex").join("AGENTS.md"));
        paths.push(home.join(".agents").join("AGENTS.md"));
        paths.push(home.join("AGENTS.md"));
    }
    paths.push(PathBuf::from("/etc/cortexfs/AGENTS.md"));
    if let Ok(cwd) = env::current_dir() {
        let mut ancestors = cwd.ancestors().map(Path::to_path_buf).collect::<Vec<_>>();
        ancestors.reverse();
        paths.extend(ancestors.into_iter().map(|path| path.join("AGENTS.md")));
    }

    collect_agent_rules_from_paths(paths)
}

#[must_use]
pub fn collect_agent_rules_from_paths(paths: impl IntoIterator<Item = PathBuf>) -> String {
    let mut output = String::new();
    let mut seen = Vec::new();
    for path in paths {
        let key = path.to_string_lossy().into_owned();
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let Some(content) = read_bounded_regular_utf8(&path, MAX_AGENT_RULE_FILE_BYTES) else {
            continue;
        };
        let section = format!("### {}\n\n{}\n\n", path.display(), content.trim());
        if output.len() + section.len() > MAX_AGENT_RULES_CHARS {
            let remaining = MAX_AGENT_RULES_CHARS.saturating_sub(output.len());
            push_str_byte_limit(&mut output, &section, remaining);
            break;
        }
        output.push_str(&section);
    }
    if output.trim().is_empty() {
        "(no AGENTS.md rules discovered)".to_owned()
    } else {
        output
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

#[must_use]
pub fn collect_skill_metadata(max_chars: usize) -> String {
    format_skill_metadata_with_budget(discover_skill_metadata(), max_chars)
}

#[must_use]
pub fn skill_metadata_budget_from_env() -> usize {
    env::var("CTX_CONTEXT_WINDOW_CHARS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .map_or(MAX_SKILL_METADATA_CHARS, |window| {
            window.saturating_mul(2).saturating_div(100)
        })
}

#[must_use]
pub fn format_skill_metadata_with_budget(
    mut skills: Vec<SkillMetadata>,
    max_chars: usize,
) -> String {
    skills.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.path.cmp(&right.path))
    });
    let full = format_skill_metadata(&skills, false);
    if full.len() <= max_chars {
        return full;
    }
    let shortened = format_skill_metadata(&skills, true);
    if shortened.len() <= max_chars {
        return format!(
            "WARNING: skill descriptions were shortened to fit the {max_chars} character budget.\n\n{shortened}"
        );
    }

    let warning = format!(
        "WARNING: skill metadata exceeded the {max_chars} character budget; some skills were omitted.\n\n"
    );
    let mut output = warning;
    for skill in &skills {
        let line = format_skill_metadata_item(skill, true);
        if output.len() + line.len() > max_chars {
            break;
        }
        output.push_str(&line);
    }
    if output.trim().is_empty() {
        "(no skills discovered)".to_owned()
    } else {
        output
    }
}

fn discover_skill_metadata() -> Vec<SkillMetadata> {
    let mut roots = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        roots.push(cwd.join(".agents").join("skills"));
        roots.push(cwd.join(".codex").join("skills"));
    }
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        roots.push(home.join(".agents").join("skills"));
        roots.push(home.join(".codex").join("skills"));
        roots.push(home.join(".codex").join("plugins").join("cache"));
    }

    let mut paths = Vec::new();
    for root in roots {
        collect_skill_files(&root, &mut paths, 0);
    }
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .filter_map(|path| read_skill_metadata(&path))
        .collect()
}

fn collect_skill_files(root: &Path, paths: &mut Vec<PathBuf>, depth: usize) {
    if depth > 8 || paths.len() >= MAX_SKILL_FILES || !is_regular_directory(root) {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if paths.len() >= MAX_SKILL_FILES {
            break;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md") {
            if !file_type.is_dir() {
                paths.push(path);
            }
        } else if file_type.is_dir() {
            collect_skill_files(&path, paths, depth + 1);
        }
    }
}

fn read_skill_metadata(path: &Path) -> Option<SkillMetadata> {
    let content = read_bounded_regular_utf8(path, MAX_SKILL_FILE_BYTES)?;
    let (name, description) = parse_skill_frontmatter(&content);
    let name = name.unwrap_or_else(|| {
        path.parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("skill")
            .to_owned()
    });
    Some(SkillMetadata {
        name,
        description: description.unwrap_or_default(),
        path: path.to_path_buf(),
    })
}

fn parse_skill_frontmatter(content: &str) -> (Option<String>, Option<String>) {
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return (None, None);
    }
    let mut name = None;
    let mut description = None;
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix("name:") {
            name = Some(value.trim().trim_matches('"').to_owned());
        } else if let Some(value) = line.strip_prefix("description:") {
            description = Some(value.trim().trim_matches('"').to_owned());
        }
    }
    (name, description)
}

fn push_str_byte_limit(output: &mut String, value: &str, max_bytes: usize) {
    if value.len() <= max_bytes {
        output.push_str(value);
        return;
    }
    let mut end = 0;
    for (index, character) in value.char_indices() {
        let next = index + character.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }
    if let Some(prefix) = value.get(..end) {
        output.push_str(prefix);
    }
}

fn is_regular_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        let file_type = metadata.file_type();
        file_type.is_dir() && !file_type.is_symlink()
    })
}

fn read_bounded_regular_utf8(path: &Path, max_bytes: u64) -> Option<String> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return None;
    }
    let mut content = String::new();
    File::open(path)
        .ok()?
        .take(max_bytes)
        .read_to_string(&mut content)
        .ok()?;
    Some(content)
}

fn format_skill_metadata(skills: &[SkillMetadata], shorten: bool) -> String {
    if skills.is_empty() {
        return "(no skills discovered)".to_owned();
    }
    let mut output = String::new();
    for skill in skills {
        output.push_str(&format_skill_metadata_item(skill, shorten));
    }
    output
}

fn format_skill_metadata_item(skill: &SkillMetadata, shorten: bool) -> String {
    let description = if shorten {
        shorten_description(&skill.description, 160)
    } else {
        skill.description.clone()
    };
    format!(
        "- name: {}\n  description: {}\n  path: {}\n",
        skill.name,
        description,
        skill.path.display()
    )
}

fn shorten_description(description: &str, max_chars: usize) -> String {
    let normalized = description.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    normalized.chars().take(max_chars).collect::<String>()
}

#[must_use]
pub fn current_time_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[must_use]
pub fn collect_history_messages_from_session(session_dir: &Path, max_chars: usize) -> String {
    let Ok(messages) = read_history_messages_tail(&session_dir.join("messages.jsonl")) else {
        return "(no historical messages injected)".to_owned();
    };
    format_history_messages_jsonl(&messages, max_chars)
}

fn read_history_messages_tail(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let read_len = len.min(MAX_HISTORY_MESSAGES_READ_BYTES);
    let start = len.saturating_sub(read_len);
    file.seek(SeekFrom::Start(start))?;

    let read_len_usize = usize::try_from(read_len)
        .map_err(|_error| std::io::Error::other("history tail too large"))?;
    let mut bytes = vec![0; read_len_usize];
    file.read_exact(&mut bytes)?;
    if start > 0
        && let Some(first_newline) = bytes.iter().position(|byte| *byte == b'\n')
    {
        bytes.drain(..=first_newline);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[must_use]
pub fn format_history_messages_jsonl(messages: &str, max_chars: usize) -> String {
    let mut rendered = VecDeque::new();
    let mut selected_len = 0;
    let mut truncated = false;

    for line in messages.lines() {
        if line.len() > MAX_HISTORY_MESSAGE_LINE_BYTES {
            continue;
        }
        let Some(line) = render_history_message_line(line) else {
            continue;
        };
        selected_len += line.len() + usize::from(!rendered.is_empty());
        rendered.push_back(line);

        while !rendered.is_empty() && selected_len > max_chars {
            let Some(removed) = rendered.pop_front() else {
                break;
            };
            truncated = true;
            selected_len = selected_len.saturating_sub(removed.len());
            if !rendered.is_empty() {
                selected_len = selected_len.saturating_sub(1);
            }
        }
    }
    if rendered.is_empty() {
        if truncated {
            return history_budget_warning(max_chars).trim_end().to_owned();
        }
        return "(no historical messages injected)".to_owned();
    }
    if !truncated {
        return rendered.into_iter().collect::<Vec<_>>().join("\n");
    }
    fit_history_lines(rendered.into_iter().collect(), max_chars)
}

fn render_history_message_line(line: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    let role = value.get("role").and_then(Value::as_str)?;
    let text = message_content_text(value.get("content"));
    if text.trim().is_empty() {
        return None;
    }
    Some(format!("- {role}: {}", text.trim()))
}

fn message_content_text(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };
    if let Some(text) = content.as_str() {
        return text.to_owned();
    }
    if let Some(parts) = content.as_array() {
        return parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| part.get("content").and_then(Value::as_str))
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    content.to_string()
}

fn history_budget_warning(max_chars: usize) -> String {
    format!(
        "WARNING: historical messages exceeded the {max_chars} character budget; oldest messages were omitted.\n\n"
    )
}

fn fit_history_lines(lines: Vec<String>, max_chars: usize) -> String {
    let warning = history_budget_warning(max_chars);
    let mut selected = Vec::new();
    let mut used = warning.len();
    for line in lines.into_iter().rev() {
        let needed = line.len() + usize::from(!selected.is_empty());
        if used + needed > max_chars {
            break;
        }
        used += needed;
        selected.push(line);
    }
    selected.reverse();
    if selected.is_empty() {
        warning.trim_end().to_owned()
    } else {
        format!("{warning}{}", selected.join("\n"))
    }
}
