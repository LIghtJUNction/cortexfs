use super::*;

pub(crate) fn run_tool(name: &str, args: &[OsString]) -> Result<ExitCode, ExecError> {
    if is_passthrough_tool(name) {
        return run_passthrough_tool(name, args).map(|()| ExitCode::SUCCESS);
    }
    let strategy = tool_invoke_strategy(name);
    if matches!(strategy, crate::tool::InvokeStrategy::Cli)
        || env::var("CTX_TOOL_MODE").as_deref() == Ok("cli")
    {
        return run_cli_tool(name, args);
    }
    let input =
        collect_input(args).map_err(|error| ExecError::with_io("cannot read input", &error))?;
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
            .map_err(|error| ExecError::with_io("cannot write output", &error)),
        Err(error) => Err(ExecError::with_io("cannot write output", &error)),
    }
}

pub(crate) fn run_cli_tool(name: &str, args: &[OsString]) -> Result<ExitCode, ExecError> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    match run_core_tool_cli(name, args, &mut stdout) {
        Ok(Some(code)) => Ok(code),
        Ok(None) => Err(ExecError::new(
            "tool is not implemented by cortexfs-object-runner",
        )),
        Err(error) => Err(ExecError::with_io("cannot run tool", &error)),
    }
}

pub(crate) fn passthrough_tool_program(name: &str) -> Option<&'static str> {
    match name {
        "bash" => Some(crate::support::command::BASH),
        "tmux" => Some(crate::support::command::TMUX),
        "zellij" => Some(crate::support::command::ZELLIJ),
        "tsh" => Some(crate::support::command::TSH),
        _ => None,
    }
}

pub(crate) fn is_passthrough_tool(name: &str) -> bool {
    passthrough_tool_program(name).is_some()
}

pub(crate) fn run_passthrough_tool(name: &str, args: &[OsString]) -> Result<(), ExecError> {
    let program = passthrough_tool_program(name).ok_or_else(|| {
        ExecError::new(format!(
            "tool is not implemented by cortexfs-object-runner: {name}"
        ))
    })?;
    let mut command = Command::new(program);
    command
        .args(args)
        .env_clear()
        .env("PATH", crate::support::command::TRUSTED_PATH);
    for key in passthrough_tool_runtime_env_keys() {
        if let Some(value) = env::var_os(key) {
            command.env(key, value);
        }
    }
    if name == "tsh" {
        for key in tsh_passthrough_capability_env_keys() {
            if let Some(value) = env::var_os(key) {
                command.env(key, value);
            }
        }
    }
    let status = command
        .status()
        .map_err(|error| ExecError::with_io(&format!("cannot run {name} tool"), &error))?;
    if status.success() {
        Ok(())
    } else {
        Err(ExecError::new(format!("{name} tool exited with {status}")))
    }
}

pub(crate) fn tsh_passthrough_capability_env_keys() -> &'static [&'static str] {
    &["CTX_CONTROL_SOCKET", "CTX_HOME", "CTX_PATH", "HOME"]
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

fn tool_invoke_strategy(name: &str) -> crate::tool::InvokeStrategy {
    let Ok(root) = env::var("CTX_ROOT") else {
        return crate::tool::InvokeStrategy::default();
    };
    crate::tool::read_invoke_strategy(&cortexfs_paths::tool_control_path(Path::new(&root), name))
}
