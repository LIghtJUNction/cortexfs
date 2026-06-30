impl Tool for ShellExecTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "shell.exec",
            description: "Run one shell command in the tool process environment.",
            input_schema: SHELL_EXEC_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let command = invocation
            .string_field("cmd")
            .unwrap_or_else(|| invocation.input().trim().to_owned());
        if command.is_empty() {
            return Err(ToolError::invalid("missing cmd"));
        }
        let command_output =
            run_shell_exec_command(&command).map_err(|error| ToolError::new("EIO", error))?;
        let mut text = String::from_utf8_lossy(&command_output.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&command_output.stderr));
        output
            .message(&text)
            .map_err(|error| ToolError::new("EIO", error.to_string()))?;
        if command_output.status.success() {
            Ok(())
        } else {
            Err(ToolError::new("EIO", "command failed"))
        }
    }
}

fn run_shell_exec_command(command: &str) -> Result<std::process::Output, String> {
    run_shell_exec_command_with_timeout(command, Duration::from_secs(SHELL_EXEC_TIMEOUT_SECONDS))
}

fn run_shell_exec_command_with_timeout(
    command: &str,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let mut child = shell_exec_command()
        .arg("-c")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|error| format!("cannot run shell command: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "cannot read shell stdout".to_owned())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "cannot read shell stderr".to_owned())?;
    let stdout_reader =
        thread::spawn(move || read_limited_bytes(stdout, MAX_SHELL_EXEC_OUTPUT_BYTES + 1));
    let stderr_reader =
        thread::spawn(move || read_limited_bytes(stderr, MAX_SHELL_EXEC_OUTPUT_BYTES + 1));
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
            if output.len() > MAX_SHELL_EXEC_OUTPUT_BYTES {
                terminate_process_group(&mut child);
                let _ignored = child.wait();
                if let Some(reader) = stderr_reader.take() {
                    let _ignored = reader.join();
                }
                return Err(format!(
                    "shell command output exceeds {MAX_SHELL_EXEC_OUTPUT_BYTES} bytes"
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
            if output.len() > MAX_SHELL_EXEC_OUTPUT_BYTES {
                terminate_process_group(&mut child);
                let _ignored = child.wait();
                if let Some(reader) = stdout_reader.take() {
                    let _ignored = reader.join();
                }
                return Err(format!(
                    "shell command output exceeds {MAX_SHELL_EXEC_OUTPUT_BYTES} bytes"
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
            if let Some(reader) = stdout_reader.take() {
                let _ignored = reader.join();
            }
            if let Some(reader) = stderr_reader.take() {
                let _ignored = reader.join();
            }
            return Err(format!(
                "shell command timed out after {}s",
                timeout.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(50));
    };
    let stdout = stdout.unwrap_or_else(|| {
        stdout_reader
            .take()
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default()
    });
    let stderr = stderr.unwrap_or_else(|| {
        stderr_reader
            .take()
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default()
    });
    if stdout.len() > MAX_SHELL_EXEC_OUTPUT_BYTES || stderr.len() > MAX_SHELL_EXEC_OUTPUT_BYTES {
        return Err(format!(
            "shell command output exceeds {MAX_SHELL_EXEC_OUTPUT_BYTES} bytes"
        ));
    }
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
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

fn run_shell_exec_cli(args: &[OsString], writer: &mut dyn Write) -> io::Result<ExitCode> {
    let command = args
        .iter()
        .map(|value| value.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    if command.is_empty() {
        writeln!(io::stderr(), "shell.exec: missing command")?;
        return Ok(ExitCode::from(2));
    }
    let output = run_shell_exec_command(&command).map_err(io::Error::other)?;
    writer.write_all(&output.stdout)?;
    io::stderr().write_all(&output.stderr)?;
    Ok(exit_code_from_status(output.status))
}

fn shell_exec_command() -> Command {
    let mut command = Command::new(SHELL_EXEC_SHELL);
    command.env_clear().env("PATH", "/usr/bin:/bin");
    command
}
