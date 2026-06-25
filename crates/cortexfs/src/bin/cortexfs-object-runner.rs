use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use cortexfs::{
    DEFAULT_AGENT_PROMPT_TEMPLATE, resolve_api_key_from_env_names, run_core_tool,
    run_core_tool_cli, run_echo_model,
};
use cortexfs_tool_sdk::ToolInvocation;
use serde_json::Value;

const DEFAULT_SOURCE: &str = "/var/lib/cortexfs/storage/v1-root";
const DEFAULT_CTX_ROOT: &str = "/ctx";
const MAX_SKILL_METADATA_CHARS: usize = 8_000;
const MAX_AGENT_RULES_CHARS: usize = 64_000;
const MAX_AGENT_RULE_FILE_BYTES: u64 = 64 * 1024;
const MAX_SKILL_FILE_BYTES: u64 = 16 * 1024;
const MAX_SKILL_FILES: usize = 256;

include!("../cortexfs_object_runner_provider.rs");

fn main() -> ExitCode {
    match run(env::args_os().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ignored = write_error(&format!("cortexfs-object-runner: {error}"));
            ExitCode::from(2)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<(), String> {
    let (object_path, input) = split_object_args(args)?;
    let object = ObjectPath::parse(&object_path)?;
    match (object.class.as_str(), object.name.as_str()) {
        ("model", name) => run_model(name, &input),
        ("agent", name) => run_agent(name, &input),
        ("tool", name) => run_tool(name, &input),
        (class, _name) => Err(format!(
            "object class {class} is not handled by this runner"
        )),
    }
}

fn run_model(name: &str, args: &[OsString]) -> Result<(), String> {
    let name = resolve_model_name(name)?;
    if name == "debug/echo" {
        let stdout = io::stdout();
        return run_echo_model(
            args.iter().map(|value| value.to_string_lossy()),
            stdout.lock(),
        )
        .map_err(|error| format!("echo model failed: {error}"));
    }
    let input = collect_input(args).map_err(|error| format!("cannot read input: {error}"))?;
    run_provider_model(&name, &input)
}

fn resolve_model_name(name: &str) -> Result<String, String> {
    if name.contains('/') {
        return Ok(name.to_owned());
    }
    let ctx_root =
        env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(DEFAULT_CTX_ROOT), PathBuf::from);
    let target = fs::read_link(ctx_root.join("model").join(name))
        .map_err(|_error| format!("missing model alias: {name}"))?;
    let Some(target) = target.to_str() else {
        return Err(format!("invalid model alias: {name}"));
    };
    target
        .strip_prefix("/ctx/model/")
        .filter(|model| model.contains('/'))
        .map(str::to_owned)
        .ok_or_else(|| format!("invalid model alias target: {name}"))
}

fn run_provider_model(name: &str, input: &str) -> Result<(), String> {
    let run = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    write_model_start(&mut stdout, &run, name)
        .map_err(|error| format!("cannot write output: {error}"))?;
    if let Err(error) = provider_chat_completion(name, input, &run, &mut stdout) {
        write_tool_error(&mut stdout, &run, "EIO", &error)
            .map_err(|error| format!("cannot write output: {error}"))?;
        return Err(error);
    }
    write_tool_done(&mut stdout, &run, "ok")
        .map_err(|error| format!("cannot write output: {error}"))
}

fn run_agent(name: &str, args: &[OsString]) -> Result<(), String> {
    let input = collect_input(args).map_err(|error| format!("cannot read input: {error}"))?;
    let source =
        env::var_os("CTX_SOURCE").map_or_else(|| PathBuf::from(DEFAULT_SOURCE), PathBuf::from);
    let ctx_root =
        env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(DEFAULT_CTX_ROOT), PathBuf::from);
    let run = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
    let model = fs::read_to_string(source.join("agent").join(format!("{name}.d")).join("model"))
        .map_or_else(
            |_error| "main".to_owned(),
            |content| content.trim().to_owned(),
        );
    let model = if model.is_empty() {
        "main".to_owned()
    } else {
        model
    };
    let model_path = ctx_root.join("model").join(&model);
    let system_prompt = fs::read_to_string(
        source
            .join("agent")
            .join(format!("{name}.d"))
            .join("system.md"),
    )
    .unwrap_or_default();
    let prompt_template = fs::read_to_string(
        source
            .join("agent")
            .join(format!("{name}.d"))
            .join("prompt.template.md"),
    )
    .unwrap_or_else(|_error| DEFAULT_AGENT_PROMPT_TEMPLATE.to_owned());
    let rules = collect_agent_rules();
    let skills = collect_skill_metadata(skill_metadata_budget());
    let current_time_unix = current_time_unix().to_string();
    if !model_path.exists() {
        let stdout = io::stdout();
        let mut stdout = stdout.lock();
        return write_tool_start(&mut stdout, &run, name)
            .and_then(|()| write_tool_error(&mut stdout, &run, "ENOENT", "missing model"))
            .map_err(|error| format!("cannot write output: {error}"));
    }
    let mut child = Command::new(model_path)
        .arg(input)
        .env("CTX_RUN_ID", &run)
        .env("CTX_AGENT", name)
        .env("CTX_AGENT_SYSTEM", system_prompt)
        .env("CTX_AGENT_PROMPT_TEMPLATE", prompt_template)
        .env("CTX_AGENT_RULES", rules)
        .env("CTX_AGENT_SKILLS", skills)
        .env("CTX_AGENT_CURRENT_TIME_UNIX", current_time_unix)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run agent model: {error}"))?;
    let child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| "cannot read agent model output".to_owned())?;
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    for line in BufReader::new(child_stdout).lines() {
        let line = line.map_err(|error| format!("cannot read agent model output: {error}"))?;
        writeln!(stdout, "{line}")
            .and_then(|()| stdout.flush())
            .map_err(|error| format!("cannot write output: {error}"))?;
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot run agent model: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("agent model failed".to_owned())
    }
}

fn run_tool(name: &str, args: &[OsString]) -> Result<(), String> {
    if is_passthrough_tool(name) {
        return run_passthrough_tool(name, args);
    }
    if env::var("CTX_TOOL_MODE").as_deref() == Ok("cli") {
        return run_cli_tool(name, args);
    }
    let input = collect_input(args).map_err(|error| format!("cannot read input: {error}"))?;
    let run = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let invocation = ToolInvocation::new(run.clone(), input);
    match run_core_tool(name, &invocation, &mut stdout) {
        Ok(true) => Ok(()),
        Ok(false) => write_tool_start(&mut stdout, &run, name)
            .and_then(|()| {
                write_tool_error(
                    &mut stdout,
                    &run,
                    "ENOSYS",
                    "tool is not implemented by cortexfs-object-runner",
                )
            })
            .map_err(|error| format!("cannot write output: {error}")),
        Err(error) => Err(format!("cannot write output: {error}")),
    }
}

fn collect_agent_rules() -> String {
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

#[derive(Clone)]
struct SkillMetadata {
    name: String,
    description: String,
    path: PathBuf,
}

fn collect_skill_metadata(max_chars: usize) -> String {
    let mut skills = discover_skill_metadata();
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

fn skill_metadata_budget() -> usize {
    env::var("CTX_CONTEXT_WINDOW_CHARS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .map_or(MAX_SKILL_METADATA_CHARS, |window| {
            window.saturating_mul(2).saturating_div(100)
        })
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
            if file_type.is_file() {
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
    output.push_str(&value[..end]);
}

fn is_regular_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| {
            let file_type = metadata.file_type();
            file_type.is_dir() && !file_type.is_symlink()
        })
        .unwrap_or(false)
}

fn read_bounded_regular_utf8(path: &Path, max_bytes: u64) -> Option<String> {
    let metadata = fs::symlink_metadata(path).ok()?;
    let file_type = metadata.file_type();
    if !file_type.is_file() || file_type.is_symlink() || metadata.len() > max_bytes {
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

fn current_time_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn run_cli_tool(name: &str, args: &[OsString]) -> Result<(), String> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    match run_core_tool_cli(name, args, &mut stdout) {
        Ok(Some(code)) if code == ExitCode::SUCCESS => Ok(()),
        Ok(Some(code)) => Err(format!("{name} tool exited with {code:?}")),
        Ok(None) => Err("tool is not implemented by cortexfs-object-runner".to_owned()),
        Err(error) => Err(format!("cannot run tool: {error}")),
    }
}

#[cfg(test)]
fn run_cli_tool_to_writer(
    name: &str,
    args: &[OsString],
    writer: &mut dyn Write,
) -> Result<(), String> {
    match run_core_tool_cli(name, args, writer) {
        Ok(Some(code)) if code == ExitCode::SUCCESS => Ok(()),
        Ok(Some(code)) => Err(format!("{name} tool exited with {code:?}")),
        Ok(None) => Err("tool is not implemented by cortexfs-object-runner".to_owned()),
        Err(error) => Err(format!("cannot run tool: {error}")),
    }
}

fn is_passthrough_tool(name: &str) -> bool {
    matches!(name, "bash" | "tmux" | "zellij" | "tsh")
}

fn run_passthrough_tool(name: &str, args: &[OsString]) -> Result<(), String> {
    let status = Command::new(name)
        .args(args)
        .status()
        .map_err(|error| format!("cannot run {name} tool: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{name} tool exited with {status}"))
    }
}

fn collect_input(args: &[OsString]) -> io::Result<String> {
    let input = args
        .iter()
        .map(|value| value.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    if !input.is_empty() {
        return Ok(input);
    }
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    Ok(input)
}

fn write_model_start(stdout: &mut impl Write, run: &str, model: &str) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"start","run":{},"model":{}}}"#,
        json_string(run),
        json_string(model)
    )
}

fn write_model_delta(stdout: &mut impl Write, run: &str, text: &str) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"delta","run":{},"text":{}}}"#,
        json_string(run),
        json_string(text)
    )
}

fn write_tool_start(stdout: &mut impl Write, run: &str, tool: &str) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"start","run":{},"tool":{}}}"#,
        json_string(run),
        json_string(tool)
    )
}

fn write_tool_done(stdout: &mut impl Write, run: &str, status: &str) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"done","run":{},"status":{}}}"#,
        json_string(run),
        json_string(status)
    )
}

fn write_tool_error(
    stdout: &mut impl Write,
    run: &str,
    code: &str,
    message: &str,
) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"error","run":{},"code":{},"message":{}}}"#,
        json_string(run),
        json_string(code),
        json_string(message)
    )?;
    write_tool_done(stdout, run, "error")
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_error| "\"\"".to_owned())
}

fn split_object_args(args: Vec<OsString>) -> Result<(PathBuf, Vec<OsString>), String> {
    let mut values = args.into_iter();
    let Some(path) = values.next() else {
        return Err("missing object path".to_owned());
    };
    Ok((PathBuf::from(path), values.collect()))
}

#[derive(Debug, Eq, PartialEq)]
struct ObjectPath {
    class: String,
    name: String,
}

impl ObjectPath {
    fn parse(path: &Path) -> Result<Self, String> {
        let leaf = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "object path has no valid name".to_owned())?;
        let parent = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .ok_or_else(|| "object path has no valid parent".to_owned())?;
        let (class, name) = if parent == "model" || parent == "agent" || parent == "tool" {
            (parent.to_owned(), leaf.to_owned())
        } else {
            let class = path
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .and_then(|value| value.to_str())
                .ok_or_else(|| "object path has no valid class".to_owned())?;
            (class.to_owned(), format!("{parent}/{leaf}"))
        };
        Ok(Self { class, name })
    }
}

fn write_error(line: &str) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    stderr
        .write_all(line.as_bytes())
        .and_then(|()| stderr.write_all(b"\n"))
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/cortexfs_object_runner_tests.rs"
    ));
}
