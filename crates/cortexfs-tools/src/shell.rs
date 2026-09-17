use crate::wait::{WaitError, wait_capped_child_output};
use crate::{SHELL_EXEC_TIMEOUT_SECONDS, shellerror::ShellExecError};
use cortexfs_tool_sdk::{Tool, ToolEmitter, ToolError, ToolInvocation, ToolResult, ToolSpec};
use std::ffi::OsString;
use std::io::{self, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, ExitCode, Output, Stdio};
use std::time::Duration;

#[derive(Debug)]
pub struct ShellExecTool;

impl Tool for ShellExecTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "shell.exec",
            description: "Run one shell command in the tool process environment.",
            input_schema: crate::configschema::SHELL_EXEC_SCHEMA,
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
        let command_output = run_shell_exec_command(&command)
            .map_err(|error| ToolError::new("EIO", error.to_string()))?;
        let mut text = String::from_utf8_lossy(&command_output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&command_output.stderr));
        output
            .message(&text)
            .map_err(|error| ToolError::new("EIO", error.to_string()))?;
        command_output
            .status
            .success()
            .then_some(())
            .ok_or_else(|| ToolError::new("EIO", "command failed"))
    }
}

pub fn run_shell_exec_command(command: &str) -> Result<Output, ShellExecError> {
    run_shell_exec_command_with_timeout(command, Duration::from_secs(SHELL_EXEC_TIMEOUT_SECONDS))
}

pub fn run_shell_exec_command_with_timeout(
    command: &str,
    timeout: Duration,
) -> Result<Output, ShellExecError> {
    let mut child = shell_exec_command()
        .arg("-c")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(ShellExecError::Spawn)?;
    wait_capped_child_output(&mut child, crate::MAX_SHELL_EXEC_OUTPUT_BYTES, timeout).map_err(
        |error| match error {
            WaitError::ExceededLimit => ShellExecError::OutputLimit {
                limit: crate::MAX_SHELL_EXEC_OUTPUT_BYTES,
            },
            WaitError::TimedOut => ShellExecError::TimedOut {
                seconds: timeout.as_secs(),
            },
            WaitError::Wait(error) => ShellExecError::Wait(error),
        },
    )
}

pub fn run_shell_exec_cli(args: &[OsString], writer: &mut dyn Write) -> io::Result<ExitCode> {
    let command = args
        .iter()
        .map(|value| value.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    if command.is_empty() {
        writeln!(io::stderr(), "shell.exec: missing command")?;
        return Ok(ExitCode::from(2));
    }
    let output =
        run_shell_exec_command(&command).map_err(|error| io::Error::other(error.to_string()))?;
    writer.write_all(&output.stdout)?;
    io::stderr().write_all(&output.stderr)?;
    let signal_code = output.status.signal().map(|signal| signal + 128);
    let code = output.status.code().or(signal_code);
    Ok(ExitCode::from(u8::try_from(code.unwrap_or(1)).unwrap_or(1)))
}

#[must_use]
pub fn shell_exec_command() -> Command {
    let mut command = Command::new(crate::SHELL_EXEC_SHELL);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_OPTIONAL_LOCKS", "0");
    command
}

#[cfg(test)]
mod tests {
    #[test]
    fn shell_exec_cli_reports_signal_exit_code() {
        let code = super::run_shell_exec_cli(&["kill -TERM $$".into()], &mut Vec::new());
        assert!(matches!(code, Ok(code) if code == std::process::ExitCode::from(143)));
    }
}
