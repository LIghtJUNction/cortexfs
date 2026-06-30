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
