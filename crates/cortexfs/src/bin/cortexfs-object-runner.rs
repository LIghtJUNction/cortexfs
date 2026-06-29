#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cortexfs::{
    DEFAULT_AGENT_PROMPT_TEMPLATE, PolicyV0, ToolExecutionAuthority, ToolExecutionDenial,
    authorize_tool_execution, collect_agent_rules, collect_skill_metadata, current_time_unix,
    derive_agent_runtime_view, inspect_event_stream_jsonl, is_model_name, is_object_name,
    parse_model_fallback, run_core_tool, run_core_tool_cli, run_echo_model,
    skill_metadata_budget_from_env,
};
use cortexfs_tool_sdk::ToolInvocation;
use nix::libc;
use serde_json::Value;

const DEFAULT_SOURCE: &str = "/var/lib/cortexfs/storage/v1-root";
const DEFAULT_CTX_ROOT: &str = "/ctx";
const MAX_AGENT_TOOL_ITERATIONS: usize = 8;
const MAX_MODEL_FALLBACK_CANDIDATES: usize = 16;
const MAX_TOOL_RESULT_CHARS: usize = 16 * 1024;
const MAX_CHILD_STDERR_BYTES: usize = 64 * 1024;
const MAX_STREAM_TOOL_CALL_BUFFER_BYTES: usize = 64 * 1024;
const MAX_AGENT_TOOL_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_AGENT_TOOL_CONTEXT_BYTES: usize = 64 * 1024;
const MAX_AGENT_TOOL_ARGC: usize = 64;
const MAX_AGENT_TOOL_ARG_BYTES: usize = 8 * 1024;
const MAX_AGENT_MODEL_FRAME_BYTES: usize = 256 * 1024;
const MAX_AGENT_MODEL_FRAMES: usize = 1024;
const MAX_RUNNER_STDIN_INPUT_BYTES: usize = 1024 * 1024;
const MAX_RUNNER_CONTROL_BYTES: u64 = 64 * 1024;
const AGENT_TOOL_TIMEOUT_SECONDS: u64 = 20;
const MAX_AGENT_TOOL_TIMEOUT_SECONDS: u64 = 120;
const AGENT_TOOL_OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);
const AGENT_MODEL_TIMEOUT_SECONDS: u64 = 120;
const MAX_AGENT_MODEL_TIMEOUT_SECONDS: u64 = 600;

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
    let target = read_model_alias_target(ctx_root, name)
        .map_err(|_error| format!("missing model alias: {name}"))?;
    let Some(model) = target.strip_prefix("/ctx/model/") else {
        return Err(format!("invalid model alias target: {name}"));
    };
    if !is_model_name(model) {
        return Err(format!("invalid model alias target: {name}"));
    }
    Ok(model.to_owned())
}

fn read_model_alias_target(ctx_root: &Path, name: &str) -> io::Result<String> {
    let model_dir = open_plain_directory(&ctx_root.join("model"))?;
    let target = nix::fcntl::readlinkat(&model_dir, name)?;
    Ok(target.to_string_lossy().into_owned())
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
    let Ok(content) = read_small_plain_text_file(&path) else {
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
    write_agent_debug_timing(&mut stdout, &config, "agent_runner_ready")?;
    if !is_regular_file_no_follow(&config.model_path) {
        let message = missing_model_message(&config.ctx_root, &config.model, &config.model_path);
        return write_tool_start(&mut stdout, &config.run, name)
            .and_then(|()| write_tool_error(&mut stdout, &config.run, "ENOENT", &message))
            .map_err(|error| format!("cannot write output: {error}"));
    }
    run_agent_tool_loop(
        &mut config,
        &input,
        &mut stdout,
        run_agent_model_once,
        execute_agent_tool_call,
    )
}

fn run_agent_tool_loop<W, M, T>(
    config: &mut AgentModelRunConfig,
    input: &str,
    stdout: &mut W,
    mut run_model_once: M,
    mut execute_tool_call: T,
) -> Result<(), String>
where
    W: Write,
    M: FnMut(&AgentModelRunConfig, &str, &mut W) -> Result<AgentModelRunOutcome, String>,
    T: FnMut(&AgentModelRunConfig, &AgentToolCall) -> Result<String, String>,
{
    let mut seen_tool_calls = BTreeSet::new();
    let mut last_tool_result: Option<(AgentToolCall, String)> = None;
    for iteration in 0..=MAX_AGENT_TOOL_ITERATIONS {
        let outcome = run_model_once(config, input, stdout)?;
        if frames_have_error(&outcome.frames)
            && let Some(pair) = last_tool_result.as_ref()
        {
            return write_tool_result_fallback_response(stdout, &config.run, &pair.0, &pair.1);
        }
        if let Some(tool_call) = first_tool_call(&outcome.frames)? {
            if !seen_tool_calls.insert(tool_call_signature(&tool_call)) {
                if let Some(pair) = last_tool_result.as_ref() {
                    return write_tool_result_fallback_response(
                        stdout,
                        &config.run,
                        &pair.0,
                        &pair.1,
                    );
                }
                return write_tool_error(
                    stdout,
                    &config.run,
                    "ELOOP",
                    "agent repeated the same tool call",
                )
                .map_err(|error| format!("cannot write output: {error}"));
            }
            write_agent_frames_for_tool_iteration(
                stdout,
                &config.run,
                &outcome.frames,
                &tool_call,
            )?;
            let result = execute_tool_call(config, &tool_call)
                .unwrap_or_else(|error| format!("ERROR: {error}\n"));
            write_tool_result_event(stdout, &config.run, &tool_call, &result)?;
            stdout
                .flush()
                .map_err(|error| format!("cannot write output: {error}"))?;
            config.push_tool_result(&tool_call, &result);
            config.suppress_model_error_events = true;
            last_tool_result = Some((tool_call, result));
            if iteration == MAX_AGENT_TOOL_ITERATIONS {
                return write_tool_error(
                    stdout,
                    &config.run,
                    "ELOOP",
                    "agent tool loop limit exceeded",
                )
                .map_err(|error| format!("cannot write output: {error}"));
            }
            continue;
        }

        if outcome.success
            && last_tool_result.is_some()
            && !frames_have_visible_assistant_response(&outcome.frames)
            && let Some(pair) = last_tool_result.as_ref()
        {
            return write_tool_result_fallback_response(stdout, &config.run, &pair.0, &pair.1);
        }

        if outcome.streamed {
            write_done_frames(stdout, &outcome.frames)?;
            if outcome.success {
                return Ok(());
            }
            if frames_have_error(&outcome.frames) {
                return Ok(());
            }
            return Err("agent model failed".to_owned());
        }
        write_agent_frames(stdout, &config.run, &outcome.frames)?;
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
    suppress_model_error_events: bool,
    debug_timing_start_unix_ms: Option<u128>,
}

impl AgentModelRunConfig {
    fn new(agent: &str) -> Result<Self, String> {
        let source =
            env::var_os("CTX_SOURCE").map_or_else(|| PathBuf::from(DEFAULT_SOURCE), PathBuf::from);
        let ctx_root =
            env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(DEFAULT_CTX_ROOT), PathBuf::from);
        let run = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
        let model_path = source
            .join("agent")
            .join(format!("{agent}.d"))
            .join("model");
        let model = read_small_plain_text_file(&model_path).map_or_else(
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
            .find(|candidate| is_regular_file_no_follow(&candidate.path))
            .or_else(|| candidates.first())
            .ok_or_else(|| format!("invalid model reference: {model}"))?;
        let model_path = selected.path.clone();
        let model = selected.name.clone();
        let agent_dir = source.join("agent").join(format!("{agent}.d"));
        let system_prompt =
            read_small_plain_text_file(&agent_dir.join("system.md")).unwrap_or_default();
        let prompt_template = read_small_plain_text_file(&agent_dir.join("prompt.template.md"))
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
            suppress_model_error_events: false,
            debug_timing_start_unix_ms: agent_debug_timing_start_unix_ms(),
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
        trim_tool_context_to_limit(&mut self.tool_context);
    }
}

fn agent_debug_timing_start_unix_ms() -> Option<u128> {
    if env::var("CTX_AGENT_DEBUG_TIMING").ok().as_deref() != Some("1") {
        return None;
    }
    env::var("CTX_AGENT_DEBUG_START_UNIX_MS")
        .ok()
        .and_then(|value| value.parse::<u128>().ok())
}

fn current_unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}

fn write_agent_debug_timing(
    stdout: &mut impl Write,
    config: &AgentModelRunConfig,
    stage: &str,
) -> Result<(), String> {
    let Some(start_unix_ms) = config.debug_timing_start_unix_ms else {
        return Ok(());
    };
    let elapsed_ms = current_unix_millis().saturating_sub(start_unix_ms);
    let frame = serde_json::json!({
        "type": "debug",
        "stage": stage,
        "elapsed_ms": elapsed_ms
    });
    writeln!(stdout, "{frame}")
        .and_then(|()| stdout.flush())
        .map_err(|error| format!("cannot write output: {error}"))
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
    run_agent_model_once_with_timeout(
        config,
        input,
        stdout,
        Duration::from_secs(agent_model_timeout_seconds()),
    )
}

fn run_agent_model_once_with_timeout(
    config: &AgentModelRunConfig,
    input: &str,
    stdout: &mut impl Write,
    timeout: Duration,
) -> Result<AgentModelRunOutcome, String> {
    write_agent_debug_timing(stdout, config, "model_spawn_start")?;
    let model_executable = open_executable_no_follow(&config.model_path)
        .map_err(|error| format!("cannot run agent model: {error}"))?;
    let mut command = Command::new(proc_fd_path(&model_executable));
    command
        .arg(input)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("CTX_ROOT", &config.ctx_root)
        .env("CTX_SOURCE", &config.source)
        .env("CTX_RUN_ID", &config.run)
        .env("CTX_AGENT", &config.agent)
        .env("CTX_AGENT_SYSTEM", &config.system_prompt)
        .env("CTX_AGENT_PROMPT_TEMPLATE", &config.prompt_template)
        .env("CTX_AGENT_RULES", &config.rules)
        .env("CTX_AGENT_SKILLS", &config.skills)
        .env("CTX_AGENT_CURRENT_TIME_UNIX", &config.current_time_unix)
        .env("CTX_AGENT_TOOL_CONTEXT", &config.tool_context);
    command.process_group(0);
    pass_runtime_provider_secret_env(&mut command);
    let mut child = spawn_with_etxtbsy_retry(command.stdout(Stdio::piped()).stderr(Stdio::piped()))
        .map_err(|error| format!("cannot run agent model: {error}"))?;
    write_agent_debug_timing(stdout, config, "model_spawned")?;
    let child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| "cannot read agent model output".to_owned())?;
    let stderr_reader = child.stderr.take().map(spawn_child_stderr_reader);
    let stdout_reader = spawn_agent_model_stdout_reader(child_stdout);
    let mut frames = Vec::new();
    let mut streamed = false;
    let mut saw_model_frame = false;
    let deadline = Instant::now() + timeout;
    loop {
        let wait = deadline
            .checked_duration_since(Instant::now())
            .map(|remaining| remaining.min(Duration::from_millis(50)))
            .unwrap_or_default();
        match stdout_reader.receiver.recv_timeout(wait) {
            Ok(Ok(line)) => {
                if !saw_model_frame {
                    write_agent_debug_timing(stdout, config, "first_model_frame")?;
                    saw_model_frame = true;
                }
                if frames.len() >= MAX_AGENT_MODEL_FRAMES {
                    let message = "agent model output frame count exceeds limit";
                    terminate_process_group(&mut child);
                    let _ignored = child.wait();
                    let _stderr = collect_child_stderr(stderr_reader);
                    let _ignored = stdout_reader.handle.join();
                    return overflow_agent_model_outcome(
                        stdout,
                        &config.run,
                        message,
                        config.suppress_model_error_events,
                    );
                }
                let line = normalize_agent_model_frame(&line, &config.run);
                if should_write_streamed_model_frame(&line, config.suppress_model_error_events) {
                    writeln!(stdout, "{line}")
                        .and_then(|()| stdout.flush())
                        .map_err(|error| {
                            terminate_process_group(&mut child);
                            let _ignored = child.wait();
                            format!("cannot write output: {error}")
                        })?;
                    streamed = true;
                }
                frames.push(line);
            }
            Ok(Err(error)) => {
                terminate_process_group(&mut child);
                let _ignored = child.wait();
                let _stderr = collect_child_stderr(stderr_reader);
                let _ignored = stdout_reader.handle.join();
                return overflow_agent_model_outcome(
                    stdout,
                    &config.run,
                    &error,
                    config.suppress_model_error_events,
                );
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if Instant::now() >= deadline {
                    let message = format!("agent model timed out after {}s", timeout.as_secs());
                    terminate_process_group(&mut child);
                    let _ignored = child.wait();
                    let _stderr = collect_child_stderr(stderr_reader);
                    let _ignored = stdout_reader.handle.join();
                    return agent_model_error_outcome(
                        stdout,
                        &config.run,
                        "ETIMEDOUT",
                        &message,
                        config.suppress_model_error_events,
                    );
                }
                let _ignored = child.try_wait().map_err(|error| error.to_string())?;
            }
        }
    }
    let _ignored = stdout_reader.handle.join();
    let status = child
        .wait()
        .map_err(|error| format!("cannot run agent model: {error}"))?;
    let stderr = collect_child_stderr(stderr_reader);
    append_model_exit_error(stdout, config, status, &stderr, &mut frames)?;
    Ok(AgentModelRunOutcome {
        frames,
        success: status.success(),
        streamed,
    })
}

fn append_model_exit_error(
    stdout: &mut impl Write,
    config: &AgentModelRunConfig,
    status: std::process::ExitStatus,
    stderr: &str,
    frames: &mut Vec<String>,
) -> Result<(), String> {
    if status.success() || frames_have_error(frames) {
        return Ok(());
    }
    let message = if stderr.trim().is_empty() {
        format!("agent model exited with {status}")
    } else {
        stderr.trim().to_owned()
    };
    if !config.suppress_model_error_events {
        write_error_event(stdout, &config.run, "EIO", &message)
            .and_then(|()| stdout.flush())
            .map_err(|error| format!("cannot write output: {error}"))?;
    }
    frames.push(
        serde_json::json!({
            "type": "error",
            "run": config.run,
            "code": "EIO",
            "message": message
        })
        .to_string(),
    );
    Ok(())
}

fn overflow_agent_model_outcome(
    stdout: &mut impl Write,
    run: &str,
    message: &str,
    suppress_output: bool,
) -> Result<AgentModelRunOutcome, String> {
    agent_model_error_outcome(stdout, run, "EOVERFLOW", message, suppress_output)
}

fn agent_model_error_outcome(
    stdout: &mut impl Write,
    run: &str,
    code: &str,
    message: &str,
    suppress_output: bool,
) -> Result<AgentModelRunOutcome, String> {
    if !suppress_output {
        write_error_event(stdout, run, code, message)
            .and_then(|()| stdout.flush())
            .map_err(|error| format!("cannot write output: {error}"))?;
    }
    Ok(AgentModelRunOutcome {
        frames: vec![
            serde_json::json!({
                "type": "error",
                "run": run,
                "code": code,
                "message": message
            })
            .to_string(),
        ],
        success: false,
        streamed: !suppress_output,
    })
}

struct AgentModelStdoutReader {
    receiver: std::sync::mpsc::Receiver<Result<String, String>>,
    handle: thread::JoinHandle<()>,
}

fn spawn_agent_model_stdout_reader(stdout: std::process::ChildStdout) -> AgentModelStdoutReader {
    let (sender, receiver) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        let mut stdout = BufReader::new(stdout);
        loop {
            match read_agent_model_frame_line(&mut stdout) {
                Ok(Some(line)) => {
                    if sender.send(Ok(line)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ignored = sender.send(Err(error));
                    break;
                }
            }
        }
    });
    AgentModelStdoutReader { receiver, handle }
}

fn spawn_with_etxtbsy_retry(command: &mut Command) -> io::Result<Child> {
    for _attempt in 0..4 {
        match command.spawn() {
            Ok(child) => return Ok(child),
            Err(error) if error.raw_os_error() == Some(libc::ETXTBSY) => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
    command.spawn()
}

fn read_agent_model_frame_line(reader: &mut impl BufRead) -> Result<Option<String>, String> {
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_AGENT_MODEL_FRAME_BYTES.saturating_add(1))
        .map_err(|_error| "agent model output frame limit is invalid".to_owned())?;
    let read = reader
        .take(limit)
        .read_until(b'\n', &mut bytes)
        .map_err(|error| format!("cannot read agent model output: {error}"))?;
    if read == 0 {
        return Ok(None);
    }
    if bytes.len() > MAX_AGENT_MODEL_FRAME_BYTES {
        return Err("agent model output frame exceeds byte limit".to_owned());
    }
    if bytes.ends_with(b"\n") {
        bytes.pop();
        if bytes.ends_with(b"\r") {
            bytes.pop();
        }
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| format!("agent model output frame is not utf-8: {error}"))
}

fn pass_runtime_provider_secret_env(command: &mut Command) {
    for name in [
        "CTX_PROVIDER_SECRET_FD",
        "CTX_PROVIDER_SECRET_PATH",
        "CTX_PROVIDER_SECRET_PROVIDER",
        "CTX_PROVIDER_SECRET_SLOT",
    ] {
        if let Some(value) = env::var_os(name) {
            command.env(name, value);
        }
    }
}

fn spawn_child_stderr_reader(mut stderr: std::process::ChildStderr) -> thread::JoinHandle<String> {
    thread::spawn(move || read_limited_text(&mut stderr, MAX_CHILD_STDERR_BYTES))
}

fn collect_child_stderr(reader: Option<thread::JoinHandle<String>>) -> String {
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

fn read_small_plain_text_file(path: &Path) -> io::Result<String> {
    let mut file = open_plain_read_file(path)?;
    let metadata = file.metadata()?;
    if metadata.len() > MAX_RUNNER_CONTROL_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds runner control read limit",
        ));
    }
    let len = usize::try_from(metadata.len()).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file is too large to read: {error}"),
        )
    })?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.utf8_error()))
}

fn open_plain_read_file(path: &Path) -> io::Result<fs::File> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;
    let parent_dir = open_plain_directory(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid file name"))?;
    // ponytail: no O_CLOEXEC here; shebang interpreters reopen /proc/self/fd/N after exec.
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW,
        nix::sys::stat::Mode::empty(),
    )?;
    let file = fs::File::from(file_fd);
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "path is not a regular file",
        ));
    }
    Ok(file)
}

fn open_plain_directory(path: &Path) -> io::Result<fs::File> {
    let mut directory = if path.is_absolute() {
        open_single_plain_directory(Path::new("/"))?
    } else {
        open_single_plain_directory(Path::new("."))?
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "invalid directory name")
                })?;
                let next = nix::fcntl::openat(
                    &directory,
                    name,
                    nix::fcntl::OFlag::O_DIRECTORY
                        | nix::fcntl::OFlag::O_RDONLY
                        | nix::fcntl::OFlag::O_NOFOLLOW
                        | nix::fcntl::OFlag::O_CLOEXEC,
                    nix::sys::stat::Mode::empty(),
                )?;
                directory = fs::File::from(next);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "directory path contains unsupported components",
                ));
            }
        }
    }
    Ok(directory)
}

fn open_single_plain_directory(path: &Path) -> io::Result<fs::File> {
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a plain directory",
        ));
    }
    Ok(directory)
}

fn is_regular_file_no_follow(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(parent_dir) = open_plain_directory(parent) else {
        return false;
    };
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Ok(file_fd) = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_PATH | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    ) else {
        return false;
    };
    fs::File::from(file_fd)
        .metadata()
        .is_ok_and(|metadata| metadata.is_file())
}

fn open_executable_no_follow(path: &Path) -> io::Result<fs::File> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;
    let parent_dir = open_plain_directory(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid file name"))?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW,
        nix::sys::stat::Mode::empty(),
    )?;
    let file = fs::File::from(file_fd);
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "path is not a regular file",
        ));
    }
    Ok(file)
}

fn proc_fd_path(file: &fs::File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
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

fn should_write_streamed_model_frame(frame: &str, suppress_error: bool) -> bool {
    match event_type(frame).as_deref() {
        Some("delta" | "reasoning_delta" | "usage") => true,
        Some("error") => !suppress_error,
        _ => false,
    }
}

fn frames_have_error(frames: &[String]) -> bool {
    frames
        .iter()
        .any(|frame| event_type(frame).as_deref() == Some("error"))
}

fn frames_have_visible_assistant_response(frames: &[String]) -> bool {
    frames.iter().any(|frame| {
        let Ok(value) = serde_json::from_str::<Value>(frame) else {
            return !frame.trim().is_empty();
        };
        match value.get("type").and_then(Value::as_str) {
            Some("delta") => value
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.trim().is_empty()),
            Some("message") if value.get("role").and_then(Value::as_str) == Some("assistant") => {
                message_has_visible_text(&value)
            }
            _ => false,
        }
    })
}

fn message_has_visible_text(value: &Value) -> bool {
    value
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("type").and_then(Value::as_str) == Some("text")
                    && item
                        .get("text")
                        .and_then(Value::as_str)
                        .is_some_and(|text| !text.trim().is_empty())
            })
        })
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
    if value.get("type").and_then(Value::as_str) != Some("delta") {
        return None;
    }
    value.get("text").and_then(Value::as_str).map(str::to_owned)
}

fn tool_call_from_text(text: &str) -> Result<Option<AgentToolCall>, String> {
    let trimmed = text.trim();
    if !trimmed.starts_with('{') {
        return Ok(None);
    }
    let mut deserializer = serde_json::Deserializer::from_str(trimmed);
    let Ok(value) = Value::deserialize(&mut deserializer) else {
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
    let args = match arguments {
        None => Vec::new(),
        Some(arguments) => {
            if let Some(args) = arguments.get("args").or_else(|| arguments.get("argv")) {
                json_string_array(args)?
            } else if let Some(command) = arguments.get("command").and_then(Value::as_str) {
                shell_words(command)?
            } else if let Some(input) = arguments.get("input").and_then(Value::as_str) {
                vec![input.to_owned()]
            } else if let Some(value) = arguments.as_str() {
                shell_words(value)?
            } else {
                return Err(
                    "tool_call arguments must contain args, argv, command, or input".to_owned(),
                );
            }
        }
    };
    validate_tool_call_arg_limits(&args)?;
    Ok(args.into_iter().map(OsString::from).collect())
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

fn validate_tool_call_arg_limits(args: &[String]) -> Result<(), String> {
    if args.len() > MAX_AGENT_TOOL_ARGC {
        return Err("tool_call args exceed argument count limit".to_owned());
    }
    let bytes = args
        .iter()
        .map(String::len)
        .try_fold(0_usize, usize::checked_add)
        .ok_or_else(|| "tool_call args exceed byte limit".to_owned())?;
    if bytes > MAX_AGENT_TOOL_ARG_BYTES {
        return Err("tool_call args exceed byte limit".to_owned());
    }
    Ok(())
}

fn validate_agent_tsh_args(args: &[OsString]) -> Result<(), String> {
    if args.is_empty() {
        return Err("tool_call args for tsh cannot be empty".to_owned());
    }
    let Some(first) = args.first() else {
        return Err("tool_call args for tsh cannot be empty".to_owned());
    };
    let Some(first) = first.to_str() else {
        return Err("tool_call args must be valid UTF-8".to_owned());
    };
    if matches!(first, "--root" | "-r") {
        return Err("tool_call args cannot override tsh root".to_owned());
    }
    if first == "tsh" {
        return Err("tool_call args for tsh must not include the tsh program name".to_owned());
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
    let policy_path = hit.control_dir().join("policy");
    let policy_text = read_small_plain_text_file(&policy_path)
        .map_err(|error| format!("cannot read {}: {error}", policy_path.display()))?;
    let tool_policy = PolicyV0::parse(&policy_text)
        .map_err(|_error| format!("invalid policy for tool:{}", tool_call.name))?;
    let grant = authorize_tool_execution(
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

    let tool_executable = open_executable_no_follow(grant.hit().path())
        .map_err(|error| format!("cannot run tool:{}: {error}", tool_call.name))?;
    let mut command = Command::new(proc_fd_path(&tool_executable));
    command
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
        .env("PATH", "/usr/bin:/bin");
    let output = run_agent_tool_process(&mut command)
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

fn run_agent_tool_process(command: &mut Command) -> Result<std::process::Output, String> {
    run_agent_tool_process_with_timeout(command, Duration::from_secs(agent_tool_timeout_seconds()))
}

fn run_agent_tool_process_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = spawn_with_etxtbsy_retry(command).map_err(|error| error.to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "cannot read tool stdout".to_owned())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "cannot read tool stderr".to_owned())?;
    let stdout_reader =
        thread::spawn(move || read_limited_bytes(stdout, MAX_AGENT_TOOL_OUTPUT_BYTES + 1));
    let stderr_reader =
        thread::spawn(move || read_limited_bytes(stderr, MAX_AGENT_TOOL_OUTPUT_BYTES + 1));
    let mut stdout_reader = Some(stdout_reader);
    let mut stderr_reader = Some(stderr_reader);
    let mut stdout = None;
    let mut stderr = None;
    let deadline = Instant::now() + timeout;
    let status = loop {
        if stdout.is_none()
            && stdout_reader
                .as_ref()
                .is_some_and(thread::JoinHandle::is_finished)
        {
            let output = stdout_reader
                .take()
                .and_then(|reader| reader.join().ok())
                .unwrap_or_default();
            if output.len() > MAX_AGENT_TOOL_OUTPUT_BYTES {
                terminate_process_group(&mut child);
                let _ignored = child.wait();
                if let Some(reader) = stderr_reader.take() {
                    let _ignored = reader.join();
                }
                return Err(format!(
                    "tool output exceeds {MAX_AGENT_TOOL_OUTPUT_BYTES} bytes"
                ));
            }
            stdout = Some(output);
        }
        if stderr.is_none()
            && stderr_reader
                .as_ref()
                .is_some_and(thread::JoinHandle::is_finished)
        {
            let output = stderr_reader
                .take()
                .and_then(|reader| reader.join().ok())
                .unwrap_or_default();
            if output.len() > MAX_AGENT_TOOL_OUTPUT_BYTES {
                terminate_process_group(&mut child);
                let _ignored = child.wait();
                if let Some(reader) = stdout_reader.take() {
                    let _ignored = reader.join();
                }
                return Err(format!(
                    "tool output exceeds {MAX_AGENT_TOOL_OUTPUT_BYTES} bytes"
                ));
            }
            stderr = Some(output);
        }
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            terminate_process_group(&mut child);
            let _ignored = child.wait();
            return Err(format!("tool timed out after {}s", timeout.as_secs()));
        }
        thread::sleep(Duration::from_millis(50));
    };
    terminate_process_group(&mut child);
    let stdout = match stdout {
        Some(output) => output,
        None => {
            collect_agent_tool_output_reader(stdout_reader.take(), AGENT_TOOL_OUTPUT_DRAIN_TIMEOUT)?
        }
    };
    let stderr = match stderr {
        Some(output) => output,
        None => {
            collect_agent_tool_output_reader(stderr_reader.take(), AGENT_TOOL_OUTPUT_DRAIN_TIMEOUT)?
        }
    };
    if stdout.len() > MAX_AGENT_TOOL_OUTPUT_BYTES || stderr.len() > MAX_AGENT_TOOL_OUTPUT_BYTES {
        return Err(format!(
            "tool output exceeds {MAX_AGENT_TOOL_OUTPUT_BYTES} bytes"
        ));
    }
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

fn collect_agent_tool_output_reader(
    reader: Option<thread::JoinHandle<Vec<u8>>>,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let Some(reader) = reader else {
        return Ok(Vec::new());
    };
    let deadline = Instant::now() + timeout;
    while !reader.is_finished() {
        if Instant::now() >= deadline {
            return Err(format!(
                "tool output did not close within {}s",
                timeout.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(reader.join().unwrap_or_default())
}

fn agent_tool_timeout_seconds() -> u64 {
    agent_tool_timeout_seconds_from_env(|name| env::var(name).ok())
}

fn agent_model_timeout_seconds() -> u64 {
    agent_model_timeout_seconds_from_env(|name| env::var(name).ok())
}

fn agent_tool_timeout_seconds_from_env(get_env: impl Fn(&str) -> Option<String>) -> u64 {
    get_env("CTX_AGENT_TOOL_TIMEOUT_SECONDS")
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (1..=MAX_AGENT_TOOL_TIMEOUT_SECONDS).contains(value))
        .unwrap_or(AGENT_TOOL_TIMEOUT_SECONDS)
}

fn agent_model_timeout_seconds_from_env(get_env: impl Fn(&str) -> Option<String>) -> u64 {
    get_env("CTX_AGENT_MODEL_TIMEOUT_SECONDS")
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (1..=MAX_AGENT_MODEL_TIMEOUT_SECONDS).contains(value))
        .unwrap_or(AGENT_MODEL_TIMEOUT_SECONDS)
}

fn read_limited_bytes(mut reader: impl Read, limit: usize) -> Vec<u8> {
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
        if output.len() >= limit {
            break;
        }
    }
    output
}

fn terminate_process_group(child: &mut Child) {
    if let Ok(pid) = i32::try_from(child.id()) {
        signal_process_group(pid, nix::sys::signal::Signal::SIGTERM);
        for _attempt in 0..5 {
            let _ignored = child.try_wait();
            thread::sleep(Duration::from_millis(50));
        }
        signal_process_group(pid, nix::sys::signal::Signal::SIGKILL);
    }
    let _ignored = child.kill();
}

fn signal_process_group(pid: i32, signal: nix::sys::signal::Signal) {
    let _ignored = nix::sys::signal::kill(nix::unistd::Pid::from_raw(-pid), signal);
}

fn tool_denial_message(name: &str, denial: ToolExecutionDenial) -> String {
    format!("cannot execute tool:{name}: {}", denial.errno())
}

fn trim_tool_result(result: &str) -> String {
    let mut result = result.to_owned();
    if result.len() > MAX_TOOL_RESULT_CHARS {
        let marker = "\n[truncated]\n";
        let mut end = MAX_TOOL_RESULT_CHARS.saturating_sub(marker.len());
        while !result.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        result.truncate(end);
        result.push_str(marker);
    }
    result
}

fn trim_tool_context_to_limit(context: &mut String) {
    if context.len() <= MAX_AGENT_TOOL_CONTEXT_BYTES {
        return;
    }
    let marker = "[earlier tool context truncated]\n\n";
    let budget = MAX_AGENT_TOOL_CONTEXT_BYTES.saturating_sub(marker.len());
    let mut start = context.len().saturating_sub(budget);
    while !context.is_char_boundary(start) {
        start = start.saturating_add(1);
    }
    let tail = context.get(start..).unwrap_or_default();
    if let Some(offset) = tail.find("\n\nTool result ") {
        start = start.saturating_add(offset).saturating_add(2);
    }
    let mut trimmed = String::with_capacity(marker.len() + context.len().saturating_sub(start));
    trimmed.push_str(marker);
    trimmed.push_str(context.get(start..).unwrap_or_default());
    if trimmed.len() > MAX_AGENT_TOOL_CONTEXT_BYTES {
        let mut retry_start = trimmed.len().saturating_sub(MAX_AGENT_TOOL_CONTEXT_BYTES);
        while !trimmed.is_char_boundary(retry_start) {
            retry_start = retry_start.saturating_add(1);
        }
        let tail = trimmed.get(retry_start..).unwrap_or_default().to_owned();
        trimmed.clear();
        trimmed.push_str(&tail);
    }
    *context = trimmed;
}

fn write_agent_frames(stdout: &mut impl Write, run: &str, frames: &[String]) -> Result<(), String> {
    for frame in frames {
        if event_type(frame).is_some() {
            writeln!(stdout, "{frame}")
                .and_then(|()| stdout.flush())
                .map_err(|error| format!("cannot write output: {error}"))?;
        } else {
            write_model_text_or_tool_call(stdout, run, frame)
                .and_then(|()| stdout.flush())
                .map_err(|error| format!("cannot write output: {error}"))?;
        }
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

fn write_tool_result_fallback_response(
    stdout: &mut impl Write,
    run: &str,
    tool_call: &AgentToolCall,
    result: &str,
) -> Result<(), String> {
    let text = format!("工具 `{}` 已执行，输出：\n\n{}", tool_call.name, result);
    let message = serde_json::json!({
        "type": "message",
        "run": run,
        "role": "assistant",
        "content": [{
            "type": "text",
            "text": text
        }]
    })
    .to_string();
    let done = serde_json::json!({
        "type": "done",
        "run": run,
        "status": "ok"
    })
    .to_string();
    writeln!(stdout, "{message}")
        .and_then(|()| writeln!(stdout, "{done}"))
        .and_then(|()| stdout.flush())
        .map_err(|error| format!("cannot write output: {error}"))
}

fn missing_model_message(ctx_root: &Path, model: &str, model_path: &Path) -> String {
    if is_model_alias(model)
        && let Ok(target) = read_model_alias_target(ctx_root, model)
    {
        return format!("missing model: {model} -> {target}");
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

fn passthrough_tool_program(name: &str) -> Option<&'static str> {
    match name {
        "bash" => Some("/usr/bin/bash"),
        "tmux" => Some("/usr/bin/tmux"),
        "zellij" => Some("/usr/bin/zellij"),
        "tsh" => Some("/usr/bin/tsh"),
        _ => None,
    }
}

fn is_passthrough_tool(name: &str) -> bool {
    passthrough_tool_program(name).is_some()
}

fn run_passthrough_tool(name: &str, args: &[OsString]) -> Result<(), String> {
    let program = passthrough_tool_program(name)
        .ok_or_else(|| format!("tool is not implemented by cortexfs-object-runner: {name}"))?;
    let mut command = Command::new(program);
    command.args(args).env_clear().env("PATH", "/usr/bin:/bin");
    for key in passthrough_tool_runtime_env_keys() {
        if let Some(value) = env::var_os(key) {
            command.env(key, value);
        }
    }
    let status = command
        .status()
        .map_err(|error| format!("cannot run {name} tool: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{name} tool exited with {status}"))
    }
}

fn passthrough_tool_runtime_env_keys() -> &'static [&'static str] {
    &[
        "CTX_AGENT",
        "CTX_ROOT",
        "CTX_SOURCE",
        "CTX_TOOL_MODE",
        "CTX_AUTHORIZED_OBJECT",
    ]
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
    read_runner_stdin_limited(io::stdin(), MAX_RUNNER_STDIN_INPUT_BYTES)
}

fn read_runner_stdin_limited(reader: impl Read, max_bytes: usize) -> io::Result<String> {
    let limit = u64::try_from(max_bytes.saturating_add(1)).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("stdin read limit is invalid: {error}"),
        )
    })?;
    let mut input = String::new();
    reader.take(limit).read_to_string(&mut input)?;
    if input.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "stdin exceeds runner input limit",
        ));
    }
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
    let path = PathBuf::from(path);
    let object_path = object_path_from_exec_metadata(&path)
        .map(|metadata_path| validate_exec_metadata_object_path(&path, metadata_path))
        .transpose()?
        .unwrap_or(path);
    Ok((object_path, values.collect()))
}

fn validate_exec_metadata_object_path(
    exec_path: &Path,
    metadata_path: PathBuf,
) -> Result<PathBuf, String> {
    let Some(authorized_path) = env::var_os("CTX_AUTHORIZED_OBJECT") else {
        return Ok(metadata_path);
    };
    let authorized_path = PathBuf::from(authorized_path);
    if metadata_path == authorized_path {
        return Ok(metadata_path);
    }
    Err(format!(
        "executable metadata object {} does not match authorized object {} for {}",
        metadata_path.display(),
        authorized_path.display(),
        exec_path.display()
    ))
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

fn object_path_from_exec_metadata(path: &Path) -> Option<PathBuf> {
    let content = read_exec_metadata_text(path)?;
    let mut class = None;
    let mut name = None;
    for line in content.lines().take(32) {
        let Some(field) = line.strip_prefix("# cortexfs.") else {
            continue;
        };
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        match key {
            "object" => class = Some(value.trim()),
            "name" => name = Some(value.trim()),
            _ => {}
        }
    }
    let class = class?;
    let name = name?;
    match class {
        "model" if is_model_name(name) => Some(Path::new("/ctx").join(class).join(name)),
        "agent" | "tool" if is_object_name(name) => Some(Path::new("/ctx").join(class).join(name)),
        _ => None,
    }
}

fn read_exec_metadata_text(path: &Path) -> Option<String> {
    let path_text = path.to_string_lossy();
    if !path_text.starts_with("/proc/self/fd/") && !path_text.starts_with("/dev/fd/") {
        return read_small_plain_text_file(path).ok();
    }
    let file = fs::File::open(path).ok()?;
    let mut content = String::new();
    file.take(MAX_RUNNER_CONTROL_BYTES.saturating_add(1))
        .read_to_string(&mut content)
        .ok()?;
    (u64::try_from(content.len()).ok()? <= MAX_RUNNER_CONTROL_BYTES).then_some(content)
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
