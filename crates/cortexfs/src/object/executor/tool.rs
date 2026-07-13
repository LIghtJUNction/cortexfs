use super::*;

pub(crate) fn run_tool(name: &str, args: &[OsString]) -> Result<ExitCode, String> {
    if is_passthrough_tool(name) {
        return run_passthrough_tool(name, args).map(|()| ExitCode::SUCCESS);
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
        Ok(true) => Ok(ExitCode::SUCCESS),
        Ok(false) => write_tool_start(&mut stdout, &run, name)
            .and_then(|()| {
                write_tool_error(
                    &mut stdout,
                    &run,
                    "ENOSYS",
                    "tool is not implemented by cortexfs-object-runner",
                )
            })
            .map(|()| ExitCode::SUCCESS)
            .map_err(|error| format!("cannot write output: {error}")),
        Err(error) => Err(format!("cannot write output: {error}")),
    }
}

pub(crate) fn run_cli_tool(name: &str, args: &[OsString]) -> Result<ExitCode, String> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    match run_core_tool_cli(name, args, &mut stdout) {
        Ok(Some(code)) => Ok(code),
        Ok(None) => Err("tool is not implemented by cortexfs-object-runner".to_owned()),
        Err(error) => Err(format!("cannot run tool: {error}")),
    }
}

pub(crate) fn passthrough_tool_program(name: &str) -> Option<&'static str> {
    match name {
        "bash" => Some("/usr/bin/bash"),
        "tmux" => Some("/usr/bin/tmux"),
        "zellij" => Some("/usr/bin/zellij"),
        "tsh" => Some("/usr/bin/tsh"),
        _ => None,
    }
}

pub(crate) fn is_passthrough_tool(name: &str) -> bool {
    passthrough_tool_program(name).is_some()
}

pub(crate) fn run_passthrough_tool(name: &str, args: &[OsString]) -> Result<(), String> {
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

pub(crate) fn passthrough_tool_runtime_env_keys() -> &'static [&'static str] {
    &[
        "CTX_AGENT",
        "CTX_SESSION",
        "CTX_RUN_ID",
        "CTX_ROOT",
        "CTX_SOURCE",
        "CTX_TOOL_MODE",
        "CTX_AUTHORIZED_OBJECT",
    ]
}

pub(crate) fn collect_input(args: &[OsString]) -> io::Result<String> {
    let input = args
        .iter()
        .map(|value| value.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    if !input.is_empty() {
        return Ok(input);
    }
    read_limited_input_text(
        io::stdin(),
        MAX_RUNNER_STDIN_INPUT_BYTES,
        "stdin exceeds runner input limit",
    )
}
