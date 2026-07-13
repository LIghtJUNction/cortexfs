#[cfg(test)]
mod tests {
    use std::process::{Command, Output};

    #[test]
    fn cli_tool_error_returns_tool_status_without_runner_stderr()
    -> Result<(), Box<dyn std::error::Error>> {
        let output = run_runner("/ctx/tool/agent.create", &["bad"], "runner-error")?;

        assert_eq!(
            (
                output.status.code(),
                String::from_utf8(output.stdout)?,
                output.stderr,
            ),
            (
                Some(1),
                concat!(
                    "{\"run\":\"runner-error\",\"tool\":\"agent.create\",\"type\":\"start\"}\n",
                    "{\"code\":\"EINVAL\",\"message\":\"invalid json input\",\"run\":\"runner-error\",\"type\":\"error\"}\n",
                    "{\"run\":\"runner-error\",\"status\":\"error\",\"type\":\"done\"}\n",
                )
                .to_owned(),
                Vec::new(),
            )
        );
        Ok(())
    }

    #[test]
    fn cli_unknown_tool_remains_runner_dispatch_error() -> Result<(), Box<dyn std::error::Error>> {
        let output = run_runner("/ctx/tool/missing", &[], "runner-missing")?;

        assert_eq!(
            (output.status.code(), output.stdout, output.stderr),
            (
                Some(2),
                Vec::new(),
                b"cortexfs-object-runner: tool is not implemented by cortexfs-object-runner\n"
                    .to_vec(),
            )
        );
        Ok(())
    }

    fn run_runner(path: &str, args: &[&str], run_id: &str) -> Result<Output, std::io::Error> {
        Command::new(env!("CARGO_BIN_EXE_cortexfs-object-runner"))
            .arg(path)
            .args(args)
            .env_clear()
            .env("CTX_TOOL_MODE", "cli")
            .env("CTX_RUN_ID", run_id)
            .output()
    }
}
