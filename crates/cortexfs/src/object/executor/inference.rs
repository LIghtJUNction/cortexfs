use super::*;

pub(crate) fn run_agent_model_once(
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

pub(crate) fn run_agent_model_once_with_timeout(
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
        .env("CTX_AGENT_TOOL_CONTEXT", &config.tool_context)
        .env("CTX_AGENT_HISTORY_MESSAGES", &config.history_messages);
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

pub(crate) fn append_model_exit_error(
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

pub(crate) fn overflow_agent_model_outcome(
    stdout: &mut impl Write,
    run: &str,
    message: &str,
    suppress_output: bool,
) -> Result<AgentModelRunOutcome, String> {
    agent_model_error_outcome(stdout, run, "EOVERFLOW", message, suppress_output)
}

pub(crate) fn agent_model_error_outcome(
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
