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
