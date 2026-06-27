use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use cortexfs::{
    DEFAULT_AGENT_PROMPT_TEMPLATE, PolicyV0, ToolExecutionAuthority, ToolExecutionDenial,
    authorize_tool_execution, collect_agent_rules, collect_skill_metadata, current_time_unix,
    derive_agent_runtime_view, inspect_event_stream_jsonl, is_model_name, is_object_name,
    parse_model_fallback, run_core_tool, run_core_tool_cli, run_echo_model,
    skill_metadata_budget_from_env,
};
use cortexfs_tool_sdk::ToolInvocation;
use serde_json::Value;

const DEFAULT_SOURCE: &str = "/var/lib/cortexfs/storage/v1-root";
const DEFAULT_CTX_ROOT: &str = "/ctx";
const MAX_AGENT_TOOL_ITERATIONS: usize = 8;
const MAX_MODEL_FALLBACK_CANDIDATES: usize = 16;
const MAX_TOOL_RESULT_CHARS: usize = 16 * 1024;
const MAX_CHILD_STDERR_BYTES: usize = 64 * 1024;
const MAX_STREAM_TOOL_CALL_BUFFER_BYTES: usize = 64 * 1024;

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
    if is_model_name(name) {
        return Ok(name.to_owned());
    }
    if !is_model_alias(name) {
        return Err(format!("invalid model reference: {name}"));
    }
    let ctx_root =
        env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(DEFAULT_CTX_ROOT), PathBuf::from);
    resolve_model_alias(&ctx_root, name)
}

fn resolve_model_alias(ctx_root: &Path, name: &str) -> Result<String, String> {
    let target = fs::read_link(ctx_root.join("model").join(name))
        .map_err(|_error| format!("missing model alias: {name}"))?;
    let Some(target) = target.to_str() else {
        return Err(format!("invalid model alias: {name}"));
    };
    let Some(model) = target.strip_prefix("/ctx/model/") else {
        return Err(format!("invalid model alias target: {name}"));
    };
    if !is_model_name(model) {
        return Err(format!("invalid model alias target: {name}"));
    }
    Ok(model.to_owned())
}

fn is_model_alias(name: &str) -> bool {
    matches!(name, "main" | "helper")
}

#[cfg(test)]
fn resolved_model_path(ctx_root: &Path, model: &str) -> Result<PathBuf, String> {
    let name = resolved_model_name(ctx_root, model)?;
    Ok(ctx_root.join("model").join(name))
}

fn resolved_model_name(ctx_root: &Path, model: &str) -> Result<String, String> {
    Ok(if is_model_name(model) {
        model.to_owned()
    } else if is_model_alias(model) {
        resolve_model_alias(ctx_root, model)?
    } else {
        return Err(format!("invalid model reference: {model}"));
    })
}

fn run_provider_model(name: &str, input: &str) -> Result<(), String> {
    let run = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
    let ctx_root =
        env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(DEFAULT_CTX_ROOT), PathBuf::from);
    let candidates = model_candidates(&ctx_root, name)?;
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let mut last_error = None;
    for candidate in candidates {
        write_model_start(&mut stdout, &run, &candidate.name)
            .map_err(|error| format!("cannot write output: {error}"))?;
        let result = if candidate.name == "debug/echo" {
            write_model_delta(&mut stdout, &run, input)
                .map_err(|error| format!("cannot write output: {error}"))
                .map_err(ProviderCompletionError::no_fallback)
        } else {
            provider_chat_completion(&candidate.name, input, &run, &mut stdout)
        };
        match result {
            Ok(()) => {
                return write_tool_done(&mut stdout, &run, "ok")
                    .map_err(|error| format!("cannot write output: {error}"));
            }
            Err(error) => {
                let can_fallback = error.can_fallback;
                last_error = Some(error.message);
                if !can_fallback {
                    break;
                }
            }
        }
    }
    let error = last_error.unwrap_or_else(|| format!("missing model: {name}"));
    write_tool_error(&mut stdout, &run, "EIO", &error)
        .map_err(|error| format!("cannot write output: {error}"))?;
    Err(error)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ModelCandidate {
    name: String,
    path: PathBuf,
}

fn model_candidates(ctx_root: &Path, model: &str) -> Result<Vec<ModelCandidate>, String> {
    let primary = resolved_model_name(ctx_root, model)?;
    let mut names = Vec::new();
    let mut seen = std::collections::HashSet::new();
    push_model_candidate_name(&primary, &mut names, &mut seen);
    for fallback in model_fallback_chain(ctx_root, &primary) {
        push_model_candidate_name(&fallback, &mut names, &mut seen);
        if names.len() >= MAX_MODEL_FALLBACK_CANDIDATES {
            break;
        }
    }
    Ok(names
        .into_iter()
        .map(|name| ModelCandidate {
            path: ctx_root.join("model").join(&name),
            name,
        })
        .collect())
}

fn push_model_candidate_name(
    name: &str,
    names: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
    if is_model_name(name) && seen.insert(name.to_owned()) {
        names.push(name.to_owned());
    }
}

fn model_fallback_chain(ctx_root: &Path, model: &str) -> Vec<String> {
    let Some((provider, name)) = model.split_once('/') else {
        return Vec::new();
    };
    let path = ctx_root
        .join("model")
        .join(provider)
        .join(format!("{name}.d"))
        .join("fallback");
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let (fallback, report) = parse_model_fallback(&content);
    if report.is_ok() {
        fallback.models().to_vec()
    } else {
        Vec::new()
    }
}

fn run_agent(name: &str, args: &[OsString]) -> Result<(), String> {
    let input = collect_input(args).map_err(|error| format!("cannot read input: {error}"))?;
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let mut config = AgentModelRunConfig::new(name)?;
    if !config.model_path.exists() {
        let message = missing_model_message(&config.ctx_root, &config.model, &config.model_path);
        return write_tool_start(&mut stdout, &config.run, name)
            .and_then(|()| write_tool_error(&mut stdout, &config.run, "ENOENT", &message))
            .map_err(|error| format!("cannot write output: {error}"));
    }
    let mut seen_tool_calls = BTreeSet::new();
    for iteration in 0..=MAX_AGENT_TOOL_ITERATIONS {
        let outcome = run_agent_model_once(&config, &input, &mut stdout)?;
        if let Some(tool_call) = first_tool_call(&outcome.frames)? {
            if !seen_tool_calls.insert(tool_call_signature(&tool_call)) {
                return write_tool_error(
                    &mut stdout,
                    &config.run,
                    "ELOOP",
                    "agent repeated the same tool call",
                )
                .map_err(|error| format!("cannot write output: {error}"));
            }
            write_agent_frames_for_tool_iteration(
                &mut stdout,
                &config.run,
                &outcome.frames,
                &tool_call,
            )?;
            let result = execute_agent_tool_call(&config, &tool_call)
                .unwrap_or_else(|error| format!("ERROR: {error}\n"));
            write_tool_result_event(&mut stdout, &config.run, &tool_call, &result)?;
            stdout
                .flush()
                .map_err(|error| format!("cannot write output: {error}"))?;
            config.push_tool_result(&tool_call, &result);
            if iteration == MAX_AGENT_TOOL_ITERATIONS {
                return write_tool_error(
                    &mut stdout,
                    &config.run,
                    "ELOOP",
                    "agent tool loop limit exceeded",
                )
                .map_err(|error| format!("cannot write output: {error}"));
            }
            continue;
        }

        if outcome.streamed {
            write_done_frames(&mut stdout, &outcome.frames)?;
            if outcome.success {
                return Ok(());
            }
            if frames_have_error(&outcome.frames) {
                return Ok(());
            }
            return Err("agent model failed".to_owned());
        }
        write_agent_frames(&mut stdout, &outcome.frames)?;
        if outcome.success {
            return Ok(());
        }
        if frames_have_error(&outcome.frames) {
            return Ok(());
        }
        return Err("agent model failed".to_owned());
    }

    Ok(())
}

struct AgentModelRunConfig {
    agent: String,
    source: PathBuf,
    ctx_root: PathBuf,
    run: String,
    model: String,
    model_path: PathBuf,
    system_prompt: String,
    prompt_template: String,
    rules: String,
    skills: String,
    current_time_unix: String,
    tool_context: String,
}

impl AgentModelRunConfig {
    fn new(agent: &str) -> Result<Self, String> {
        let source =
            env::var_os("CTX_SOURCE").map_or_else(|| PathBuf::from(DEFAULT_SOURCE), PathBuf::from);
        let ctx_root =
            env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(DEFAULT_CTX_ROOT), PathBuf::from);
        let run = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
        let model = fs::read_to_string(
            source
                .join("agent")
                .join(format!("{agent}.d"))
                .join("model"),
        )
        .map_or_else(
            |_error| "main".to_owned(),
            |content| content.trim().to_owned(),
        );
        let model = if model.is_empty() {
            "main".to_owned()
        } else {
            model
        };
        let candidates = model_candidates(&ctx_root, &model)?;
        let selected = candidates
            .iter()
            .find(|candidate| candidate.path.exists())
            .or_else(|| candidates.first())
            .ok_or_else(|| format!("invalid model reference: {model}"))?;
        let model_path = selected.path.clone();
        let model = selected.name.clone();
        let system_prompt = fs::read_to_string(
            source
                .join("agent")
                .join(format!("{agent}.d"))
                .join("system.md"),
        )
        .unwrap_or_default();
        let prompt_template = fs::read_to_string(
            source
                .join("agent")
                .join(format!("{agent}.d"))
                .join("prompt.template.md"),
        )
        .unwrap_or_else(|_error| DEFAULT_AGENT_PROMPT_TEMPLATE.to_owned());
        Ok(Self {
            agent: agent.to_owned(),
            source,
            ctx_root,
            run,
            model,
            model_path,
            system_prompt,
            prompt_template,
            rules: collect_agent_rules(),
            skills: collect_skill_metadata(skill_metadata_budget_from_env()),
            current_time_unix: current_time_unix().to_string(),
            tool_context: env::var("CTX_AGENT_TOOL_CONTEXT").unwrap_or_default(),
        })
    }

    fn push_tool_result(&mut self, tool_call: &AgentToolCall, result: &str) {
        if !self.tool_context.trim().is_empty() {
            self.tool_context.push_str("\n\n");
        }
        self.tool_context.push_str("Tool result ");
        self.tool_context.push_str(&tool_call.id);
        self.tool_context.push_str(" from ");
        self.tool_context.push_str(&tool_call.name);
        self.tool_context.push_str(":\n");
        self.tool_context.push_str(result);
    }
}

struct AgentModelRunOutcome {
    frames: Vec<String>,
    success: bool,
    streamed: bool,
}

fn run_agent_model_once(
    config: &AgentModelRunConfig,
    input: &str,
    stdout: &mut impl Write,
) -> Result<AgentModelRunOutcome, String> {
    let mut child = Command::new(&config.model_path)
        .arg(input)
        .env("CTX_RUN_ID", &config.run)
        .env("CTX_AGENT", &config.agent)
        .env("CTX_AGENT_SYSTEM", &config.system_prompt)
        .env("CTX_AGENT_PROMPT_TEMPLATE", &config.prompt_template)
        .env("CTX_AGENT_RULES", &config.rules)
        .env("CTX_AGENT_SKILLS", &config.skills)
        .env("CTX_AGENT_CURRENT_TIME_UNIX", &config.current_time_unix)
        .env("CTX_AGENT_TOOL_CONTEXT", &config.tool_context)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run agent model: {error}"))?;
    let child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| "cannot read agent model output".to_owned())?;
    let stderr_reader = child.stderr.take().map(spawn_child_stderr_reader);
    let mut frames = Vec::new();
    let mut streamed = false;
    for line in BufReader::new(child_stdout).lines() {
        let line = line.map_err(|error| format!("cannot read agent model output: {error}"))?;
        let line = normalize_agent_model_frame(&line, &config.run);
        if matches!(
            event_type(&line).as_deref(),
            Some("delta" | "reasoning_delta" | "usage" | "error")
        ) {
            writeln!(stdout, "{line}")
                .and_then(|()| stdout.flush())
                .map_err(|error| format!("cannot write output: {error}"))?;
            streamed = true;
        }
        frames.push(line);
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot run agent model: {error}"))?;
    let stderr = collect_child_stderr(stderr_reader);
    if !status.success() && !frames_have_error(&frames) {
        let message = if stderr.trim().is_empty() {
            format!("agent model exited with {status}")
        } else {
            stderr.trim().to_owned()
        };
        write_error_event(stdout, &config.run, "EIO", &message)
            .and_then(|()| stdout.flush())
            .map_err(|error| format!("cannot write output: {error}"))?;
        frames.push(
            serde_json::json!({
                "type": "error",
                "run": config.run,
                "code": "EIO",
                "message": message
            })
            .to_string(),
        );
    }
    Ok(AgentModelRunOutcome {
        frames,
        success: status.success(),
        streamed,
    })
}

fn spawn_child_stderr_reader(
    mut stderr: std::process::ChildStderr,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || read_limited_text(&mut stderr, MAX_CHILD_STDERR_BYTES))
}

fn collect_child_stderr(reader: Option<std::thread::JoinHandle<String>>) -> String {
    let Some(reader) = reader else {
        return String::new();
    };
    reader.join().unwrap_or_default()
}

fn read_limited_text(reader: &mut impl Read, limit: usize) -> String {
    let mut output = Vec::with_capacity(limit.min(8 * 1024));
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let remaining = limit.saturating_sub(output.len());
        let kept = read.min(remaining);
        if let Some(chunk) = buffer.get(..kept) {
            output.extend_from_slice(chunk);
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn normalize_agent_model_frame(frame: &str, run: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(frame) else {
        return frame.to_owned();
    };
    if value.get("type").and_then(Value::as_str) == Some("error")
        && value.get("run").is_none()
        && let Some(object) = value.as_object_mut()
    {
        object.insert("run".to_owned(), Value::String(run.to_owned()));
        return value.to_string();
    }
    frame.to_owned()
}

fn frames_have_error(frames: &[String]) -> bool {
    frames
        .iter()
        .any(|frame| event_type(frame).as_deref() == Some("error"))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AgentToolCall {
    id: String,
    name: String,
    args: Vec<OsString>,
}

fn first_tool_call(frames: &[String]) -> Result<Option<AgentToolCall>, String> {
    for frame in frames {
        if let Some(call) = tool_call_from_event_frame(frame)? {
            return Ok(Some(call));
        }
        if let Some(text) = event_text(frame)
            && let Some(call) = tool_call_from_text(&text)?
        {
            return Ok(Some(call));
        }
    }
    Ok(None)
}

fn tool_call_signature(tool_call: &AgentToolCall) -> String {
    let args = tool_call
        .args
        .iter()
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\u{1f}");
    format!("{}\u{1e}{args}", tool_call.name)
}

fn tool_call_from_event_frame(frame: &str) -> Result<Option<AgentToolCall>, String> {
    let Ok(value) = serde_json::from_str::<Value>(frame) else {
        return Ok(None);
    };
    if value.get("type").and_then(Value::as_str) != Some("tool_call") {
        return Ok(None);
    }
    agent_tool_call_from_value(&value)
}

fn event_text(frame: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(frame).ok()?;
    if !matches!(
        value.get("type").and_then(Value::as_str),
        Some("delta" | "reasoning_delta")
    ) {
        return None;
    }
    value.get("text").and_then(Value::as_str).map(str::to_owned)
}

fn tool_call_from_text(text: &str) -> Result<Option<AgentToolCall>, String> {
    let trimmed = text.trim();
    if !trimmed.starts_with('{') {
        return Ok(None);
    }
    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        return Ok(None);
    };
    if value.get("type").and_then(Value::as_str) != Some("tool_call") {
        return Ok(None);
    }
    agent_tool_call_from_value(&value)
}

fn agent_tool_call_from_value(value: &Value) -> Result<Option<AgentToolCall>, String> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "tool_call missing id".to_owned())?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "tool_call missing name".to_owned())?;
    if !is_object_name(id) {
        return Err(format!("invalid tool_call id: {id}"));
    }
    if !is_object_name(name) {
        return Err(format!("invalid tool_call name: {name}"));
    }
    let args = tool_call_args(value.get("arguments"))?;
    Ok(Some(AgentToolCall {
        id: id.to_owned(),
        name: name.to_owned(),
        args,
    }))
}

fn tool_call_args(arguments: Option<&Value>) -> Result<Vec<OsString>, String> {
    let Some(arguments) = arguments else {
        return Ok(Vec::new());
    };
    if let Some(args) = arguments.get("args").or_else(|| arguments.get("argv")) {
        return json_string_array(args)
            .map(|values| values.into_iter().map(OsString::from).collect());
    }
    if let Some(command) = arguments.get("command").and_then(Value::as_str) {
        return Ok(shell_words(command)?
            .into_iter()
            .map(OsString::from)
            .collect());
    }
    if let Some(input) = arguments.get("input").and_then(Value::as_str) {
        return Ok(vec![OsString::from(input)]);
    }
    if let Some(value) = arguments.as_str() {
        return Ok(shell_words(value)?
            .into_iter()
            .map(OsString::from)
            .collect());
    }
    Err("tool_call arguments must contain args, argv, command, or input".to_owned())
}

fn json_string_array(value: &Value) -> Result<Vec<String>, String> {
    let Some(values) = value.as_array() else {
        return Err("tool_call args must be an array".to_owned());
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "tool_call args must be strings".to_owned())
        })
        .collect()
}

fn shell_words(value: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escape = false;
    for character in value.chars() {
        if escape {
            word.push(character);
            escape = false;
            continue;
        }
        match (quote, character) {
            (_, '\\') => escape = true,
            (Some(active), candidate) if candidate == active => quote = None,
            (Some(_active), candidate) => word.push(candidate),
            (None, '\'' | '"') => quote = Some(character),
            (None, candidate) if candidate.is_whitespace() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            (None, candidate) => word.push(candidate),
        }
    }
    if escape {
        return Err("tool_call command ends with unfinished escape".to_owned());
    }
    if quote.is_some() {
        return Err("tool_call command has unterminated quote".to_owned());
    }
    if !word.is_empty() {
        words.push(word);
    }
    Ok(words)
}

fn validate_agent_tsh_args(args: &[OsString]) -> Result<(), String> {
    let Some(first) = args.first() else {
        return Ok(());
    };
    let Some(first) = first.to_str() else {
        return Err("tool_call args must be valid UTF-8".to_owned());
    };
    if matches!(first, "--root" | "-r") {
        return Err("tool_call args cannot override tsh root".to_owned());
    }
    Ok(())
}

fn execute_agent_tool_call(
    config: &AgentModelRunConfig,
    tool_call: &AgentToolCall,
) -> Result<String, String> {
    if tool_call.name != "tsh" {
        return Err(format!(
            "unsupported native tool {}; use tsh",
            tool_call.name
        ));
    }
    let view = derive_agent_runtime_view(&config.ctx_root, &config.agent)
        .map_err(|error| format!("cannot derive agent authority: {}", error.errno()))?;
    let Some(hit) = view
        .tool_path()
        .find(&tool_call.name)
        .map_err(|error| format!("cannot inspect CTX_PATH: {error:?}"))?
    else {
        return Err(format!("tool not found: {}", tool_call.name));
    };
    let policy_text = fs::read_to_string(hit.control_dir().join("policy")).map_err(|error| {
        format!(
            "cannot read {}: {error}",
            hit.control_dir().join("policy").display()
        )
    })?;
    let tool_policy = PolicyV0::parse(&policy_text)
        .map_err(|_error| format!("invalid policy for tool:{}", tool_call.name))?;
    authorize_tool_execution(
        view.tool_path(),
        &tool_call.name,
        ToolExecutionAuthority::new(
            view.identity(),
            view.mount_table(),
            view.policy_subject(),
            view.policy(),
            &tool_policy,
        ),
    )
    .map_err(|denial| tool_denial_message(&tool_call.name, denial))?;
    validate_agent_tsh_args(&tool_call.args)?;

    let output = Command::new(hit.path())
        .args(&tool_call.args)
        .env_clear()
        .envs(
            view.env()
                .iter()
                .map(|env| (env.0.as_str(), env.1.as_str())),
        )
        .env("CTX_AGENT", &config.agent)
        .env("CTX_ROOT", &config.ctx_root)
        .env("CTX_SOURCE", &config.source)
        .env("CTX_TOOL_MODE", "cli")
        .env("PATH", "/usr/bin:/bin")
        .output()
        .map_err(|error| format!("cannot run tool:{}: {error}", tool_call.name))?;
    let mut result = String::new();
    result.push_str(&String::from_utf8_lossy(&output.stdout));
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(stderr.trim_end());
        result.push('\n');
    }
    if !output.status.success() {
        if result.trim().is_empty() {
            result.push_str("tool exited with ");
            result.push_str(&output.status.to_string());
            result.push('\n');
        }
        return Err(trim_tool_result(&result));
    }
    Ok(trim_tool_result(&result))
}

fn tool_denial_message(name: &str, denial: ToolExecutionDenial) -> String {
    format!("cannot execute tool:{name}: {}", denial.errno())
}

fn trim_tool_result(result: &str) -> String {
    let mut result = result.to_owned();
    if result.len() > MAX_TOOL_RESULT_CHARS {
        result.truncate(MAX_TOOL_RESULT_CHARS);
        result.push_str("\n[truncated]\n");
    }
    result
}

fn write_agent_frames(stdout: &mut impl Write, frames: &[String]) -> Result<(), String> {
    for frame in frames {
        writeln!(stdout, "{frame}")
            .and_then(|()| stdout.flush())
            .map_err(|error| format!("cannot write output: {error}"))?;
    }
    Ok(())
}

fn write_done_frames(stdout: &mut impl Write, frames: &[String]) -> Result<(), String> {
    for frame in frames {
        if event_type(frame).as_deref() == Some("done") {
            writeln!(stdout, "{frame}")
                .and_then(|()| stdout.flush())
                .map_err(|error| format!("cannot write output: {error}"))?;
        }
    }
    Ok(())
}

fn write_agent_frames_for_tool_iteration(
    stdout: &mut impl Write,
    run: &str,
    frames: &[String],
    tool_call: &AgentToolCall,
) -> Result<(), String> {
    let mut wrote_tool_call = false;
    for frame in frames {
        if matches!(event_type(frame).as_deref(), Some("start" | "done")) {
            continue;
        }
        if tool_call_from_event_frame(frame)?.is_some() {
            writeln!(stdout, "{frame}")
                .and_then(|()| stdout.flush())
                .map_err(|error| format!("cannot write output: {error}"))?;
            wrote_tool_call = true;
        }
    }
    if !wrote_tool_call {
        write_tool_call_event(stdout, run, tool_call)
            .and_then(|()| stdout.flush())
            .map_err(|error| format!("cannot write output: {error}"))?;
    }
    Ok(())
}

fn event_type(frame: &str) -> Option<String> {
    serde_json::from_str::<Value>(frame)
        .ok()
        .and_then(|value| value.get("type").and_then(Value::as_str).map(str::to_owned))
}

fn write_tool_result_event(
    stdout: &mut impl Write,
    run: &str,
    tool_call: &AgentToolCall,
    result: &str,
) -> Result<(), String> {
    let event = serde_json::json!({
        "type": "message",
        "run": run,
        "role": "tool",
        "name": tool_call.name,
        "content": [{
            "type": "tool_result",
            "tool_call_id": tool_call.id,
            "content": result
        }]
    })
    .to_string();
    if !inspect_event_stream_jsonl(&event).is_ok() {
        return Err("generated invalid tool result event".to_owned());
    }
    writeln!(stdout, "{event}").map_err(|error| format!("cannot write output: {error}"))
}

fn missing_model_message(ctx_root: &Path, model: &str, model_path: &Path) -> String {
    if is_model_alias(model)
        && let Ok(target) = fs::read_link(ctx_root.join("model").join(model))
    {
        return format!("missing model: {model} -> {}", target.display());
    }
    format!("missing model: {}", model_path.display())
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

fn write_model_usage(stdout: &mut impl Write, run: &str, usage: TokenUsage) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"usage","run":{},"input_tokens":{},"output_tokens":{}}}"#,
        json_string(run),
        usage.input_tokens,
        usage.output_tokens
    )
}

fn write_model_text_or_tool_call(stdout: &mut impl Write, run: &str, text: &str) -> io::Result<()> {
    if let Some(tool_call) = tool_call_from_text(text).map_err(io::Error::other)? {
        return write_tool_call_event(stdout, run, &tool_call);
    }
    write_model_delta(stdout, run, text)
}

fn write_tool_call_event(
    stdout: &mut impl Write,
    run: &str,
    tool_call: &AgentToolCall,
) -> io::Result<()> {
    let args = tool_call
        .args
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let event = serde_json::json!({
        "type": "tool_call",
        "run": run,
        "id": tool_call.id,
        "name": tool_call.name,
        "arguments": {
            "args": args
        }
    })
    .to_string();
    writeln!(stdout, "{event}")
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
    write_error_event(stdout, run, code, message)?;
    write_tool_done(stdout, run, "error")
}

fn write_error_event(
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
    )
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
