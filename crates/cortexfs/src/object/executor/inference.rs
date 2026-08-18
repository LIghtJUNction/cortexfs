use super::*;

pub(crate) fn run_agent_model_once(
    config: &AgentModelRunConfig,
    input: &str,
    stdout: &mut impl Write,
) -> Result<AgentModelRunOutcome, ExecError> {
    run_agent_model_once_with_timeout(
        config,
        input,
        stdout,
        Duration::from_secs(agent_model_timeout_seconds()),
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "bounded model subprocess state machine keeps cleanup and frame order auditable"
)]
pub(crate) fn run_agent_model_once_with_timeout(
    config: &AgentModelRunConfig,
    input: &str,
    stdout: &mut impl Write,
    timeout: Duration,
) -> Result<AgentModelRunOutcome, ExecError> {
    let step = env::var("CTX_AGENT_STEP")
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or_default();
    let identity = crate::AgentUnixIdentity::new(
        nix::unistd::geteuid().as_raw(),
        nix::unistd::getegid().as_raw(),
        std::iter::empty(),
    );
    let mut hook = crate::runtime::hookabi::HookInvocation {
        phase: crate::runtime::hookabi::HookPhase::Pre,
        action: "model",
        agent: &config.agent,
        run: &config.run,
        step,
        tool: None,
        status: None,
    };
    let control_dir = agent_model_control_dir(&config.source, &config.agent);
    if let Err(error) = crate::runtime::hooks::run_agent_hooks(&control_dir, &hook, &identity) {
        return recoverable_hook_error_outcome(stdout, &config.run, &error);
    }
    if !admit_agent_prompt(config, input)? {
        return agent_model_error_outcome(
            stdout,
            &config.run,
            "E2BIG",
            "agent prompt exceeds the effective context window",
            config.suppress_model_error_events,
        );
    }
    write_agent_debug_timing(stdout, config, "model_spawn_start")?;
    let model_executable = open_executable_no_follow(&config.model_path)
        .map_err(|error| ExecError::with_io("cannot run agent model", &error))?;
    let mut command = agent_model_command(config, input, &model_executable);
    let mut child = spawn_with_etxtbsy_retry(command.stdout(Stdio::piped()).stderr(Stdio::piped()))
        .map_err(|error| ExecError::with_io("cannot run agent model", &error))?;
    write_agent_debug_timing(stdout, config, "model_spawned")?;
    let child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| ExecError::new("cannot read agent model output"))?;
    let stderr_reader = child.stderr.take().map(spawn_child_stderr_reader);
    let stdout_reader = spawn_agent_model_stdout_reader(child_stdout);
    let mut frames = Vec::new();
    let mut frame_bytes = 0usize;
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
                let line = normalize_agent_model_frame(&line, &config.run);
                let next_frame_bytes = frame_bytes.saturating_add(line.len());
                if frames.len() >= MAX_AGENT_MODEL_FRAMES
                    || next_frame_bytes > MAX_AGENT_MODEL_OUTPUT_BYTES
                {
                    let message = "agent model output exceeds configured limit";
                    terminate_process_group(&mut child);
                    let _ignored = child.wait();
                    let _stderr = collect_child_stderr(stderr_reader);
                    stdout_reader.join();
                    return overflow_agent_model_outcome(
                        stdout,
                        &config.run,
                        message,
                        config.suppress_model_error_events,
                    );
                }
                if should_write_streamed_model_frame(&line, config.suppress_model_error_events) {
                    let write_result = writeln!(stdout, "{line}")
                        .and_then(|()| stdout.flush())
                        .map_err(|error| ExecError::with_io("cannot write output", &error));
                    if let Err(error) = write_result {
                        terminate_process_group(&mut child);
                        let _ignored = child.wait();
                        let _stderr = collect_child_stderr(stderr_reader);
                        stdout_reader.join();
                        return Err(error);
                    }
                    streamed = true;
                }
                frame_bytes = next_frame_bytes;
                frames.push(line);
            }
            Ok(Err(error)) => {
                terminate_process_group(&mut child);
                let _ignored = child.wait();
                let _stderr = collect_child_stderr(stderr_reader);
                stdout_reader.join();
                return overflow_agent_model_outcome(
                    stdout,
                    &config.run,
                    error.message(),
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
                    stdout_reader.join();
                    return agent_model_error_outcome(
                        stdout,
                        &config.run,
                        "ETIMEDOUT",
                        &message,
                        config.suppress_model_error_events,
                    );
                }
                let _ignored = child
                    .try_wait()
                    .map_err(|error| ExecError::with_io("cannot run agent model", &error))?;
            }
        }
    }
    stdout_reader.join();
    let status = child
        .wait()
        .map_err(|error| ExecError::with_io("cannot run agent model", &error))?;
    let stderr = collect_child_stderr(stderr_reader);
    append_model_exit_error(stdout, config, status, &stderr, &mut frames)?;
    hook.phase = crate::runtime::hookabi::HookPhase::Post;
    hook.status = Some(if status.success() { "ok" } else { "error" });
    if let Err(error) = crate::runtime::hooks::run_agent_hooks(&control_dir, &hook, &identity) {
        frames.push(write_recoverable_hook_error(stdout, &config.run, &error)?);
    }
    let success = status.success()
        && frames.iter().all(|frame| {
            serde_json::from_str::<Value>(frame)
                .ok()
                .and_then(|value| {
                    value
                        .get("type")
                        .and_then(Value::as_str)
                        .map(|kind| kind != "error")
                })
                .unwrap_or(true)
        });
    Ok(AgentModelRunOutcome {
        frames,
        success,
        streamed,
    })
}

pub(crate) fn agent_model_command(
    config: &AgentModelRunConfig,
    input: &str,
    model_executable: &fs::File,
) -> Command {
    let mut command = Command::new(proc_fd_path(model_executable));
    command
        .arg(input)
        .env_clear()
        .env("PATH", crate::support::command::TRUSTED_PATH)
        .env("CTX_ROOT", &config.ctx_root)
        .env("CTX_SOURCE", &config.source)
        .env("CTX_RUN_ID", &config.run)
        .env("CTX_AGENT", &config.agent)
        .env("CTX_AGENT_SYSTEM", &config.system_prompt)
        .env("CTX_AGENT_PROMPT_TEMPLATE", &config.prompt_template)
        .env("CTX_AGENT_RULES", &config.rules)
        .env("CTX_AGENT_SKILLS", &config.skills)
        .env("CTX_AGENT_CURRENT_TIME_UNIX", &config.current_time_unix)
        .env("CTX_AGENT_TOOL_CONTEXT", &config.tool_context)
        .env("CTX_AGENT_HISTORY_MESSAGES", &config.history_messages);
    if let Some(budget) = config.context_budget {
        command.env("CTX_CONTEXT_WINDOW_TOKENS", budget.tokens().to_string());
        command.env("CTX_CONTEXT_WINDOW_CHARS", budget.total_chars().to_string());
    }
    command.env("CTX_AGENT_WINDOW_SETTING", config.window_setting.value());
    command.process_group(0);
    pass_runtime_provider_secret_env(&mut command);
    pass_provider_egress_env(&mut command);
    command
}

fn pass_provider_egress_env(command: &mut Command) {
    let value = env::var_os(cortexfs::runtime::egress::PROVIDER_EGRESS_DIR_ENV);
    if value.as_deref()
        == Some(OsStr::new(
            cortexfs::runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH,
        ))
    {
        command.env(
            cortexfs::runtime::egress::PROVIDER_EGRESS_DIR_ENV,
            cortexfs::runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH,
        );
    }
}

pub(crate) fn append_model_exit_error(
    stdout: &mut impl Write,
    config: &AgentModelRunConfig,
    status: std::process::ExitStatus,
    stderr: &str,
    frames: &mut Vec<String>,
) -> Result<(), ExecError> {
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
            .map_err(|error| ExecError::with_io("cannot write output", &error))?;
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

pub(crate) fn overflow_agent_model_outcome(
    stdout: &mut impl Write,
    run: &str,
    message: &str,
    suppress_output: bool,
) -> Result<AgentModelRunOutcome, ExecError> {
    agent_model_error_outcome(stdout, run, "EOVERFLOW", message, suppress_output)
}

pub(crate) fn agent_model_error_outcome(
    stdout: &mut impl Write,
    run: &str,
    code: &str,
    message: &str,
    suppress_output: bool,
) -> Result<AgentModelRunOutcome, ExecError> {
    if !suppress_output {
        write_error_event(stdout, run, code, message)
            .and_then(|()| stdout.flush())
            .map_err(|error| ExecError::with_io("cannot write output", &error))?;
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

fn recoverable_hook_error_outcome(
    stdout: &mut impl Write,
    run: &str,
    error: &crate::runtime::hookabi::HookError,
) -> Result<AgentModelRunOutcome, ExecError> {
    let frame = write_recoverable_hook_error(stdout, run, error)?;
    Ok(AgentModelRunOutcome {
        frames: vec![frame],
        success: false,
        streamed: true,
    })
}

fn write_recoverable_hook_error(
    stdout: &mut impl Write,
    run: &str,
    error: &crate::runtime::hookabi::HookError,
) -> Result<String, ExecError> {
    let frame = serde_json::json!({
        "type": "error",
        "run": run,
        "code": error.code(),
        "message": error.message(),
        "recoverable": true
    })
    .to_string();
    writeln!(stdout, "{frame}")
        .and_then(|()| stdout.flush())
        .map_err(|error| ExecError::with_io("cannot write output", &error))?;
    Ok(frame)
}
