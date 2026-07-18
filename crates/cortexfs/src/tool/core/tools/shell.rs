use super::*;
use crate::*;

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

pub(crate) fn run_shell_exec_command(command: &str) -> Result<std::process::Output, String> {
    run_shell_exec_command_with_timeout(command, Duration::from_secs(SHELL_EXEC_TIMEOUT_SECONDS))
}

pub(crate) fn run_shell_exec_command_with_timeout(
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
    support::process::wait_capped_child_output(
        &mut child,
        support::process::CappedOutputWait {
            max_output_bytes: MAX_SHELL_EXEC_OUTPUT_BYTES,
            timeout,
            capture_stderr: true,
            drain_timeout: None,
            terminate_group_after_exit: false,
        },
        || false,
    )
    .map_err(|error| match error {
        support::process::CappedOutputError::ExceededLimit => {
            format!("shell command output exceeds {MAX_SHELL_EXEC_OUTPUT_BYTES} bytes")
        }
        support::process::CappedOutputError::TimedOut => {
            format!("shell command timed out after {}s", timeout.as_secs())
        }
        support::process::CappedOutputError::Wait(error) => error.to_string(),
        support::process::CappedOutputError::Cancelled
        | support::process::CappedOutputError::DrainTimedOut => "shell command failed".to_owned(),
    })
}

pub(crate) fn run_shell_exec_cli(
    args: &[OsString],
    writer: &mut dyn Write,
) -> io::Result<ExitCode> {
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

pub(crate) fn shell_exec_command() -> Command {
    let mut command = Command::new(SHELL_EXEC_SHELL);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_OPTIONAL_LOCKS", "0");
    command
}
