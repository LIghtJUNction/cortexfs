use crate::*;

pub(crate) fn print_agent_repl_banner(
    root: &Path,
    name: &str,
    session: &str,
) -> Result<(), CliError> {
    let color = color_enabled();
    for line in agent_repl_banner_lines(color, root, name, session)? {
        write_error(&line)
            .map_err(|error| CliError::unavailable(format!("stderr write failed: {error}")))?;
    }
    Ok(())
}

pub(crate) fn agent_repl_banner_lines(
    color: bool,
    root: &Path,
    name: &str,
    session: &str,
) -> Result<Vec<String>, CliError> {
    let model_summary = agent_repl_model_summary(color, root, name)?;
    let workspace = agent_repl_workspace_line(color, root, name, session)?;
    let mut lines = vec![
        format!(
            "{} ctx agent {}/{} - {}",
            styled(color, ANSI_BOLD_CYAN, "●"),
            styled(color, ANSI_BOLD_CYAN, name),
            styled(color, ANSI_CYAN, session),
            styled(color, ANSI_CYAN, "chat shell")
        ),
        format!(
            "    {} {}",
            styled(color, ANSI_BOLD_BLUE, "Mode:"),
            styled(
                color,
                ANSI_DIM,
                "messages go to the agent; tools run inside tsh"
            )
        ),
        format!(
            "    {} {}",
            styled(color, ANSI_BOLD_BLUE, "Model:"),
            model_summary
        ),
    ];
    lines.push(workspace);
    lines.push(format!(
        " {} {}",
        styled(color, ANSI_BOLD_BLUE, "Commands:"),
        styled(color, ANSI_DIM, AGENT_REPL_COMMANDS)
    ));
    Ok(lines)
}

pub(crate) fn agent_repl_workspace_line(
    color: bool,
    root: &Path,
    name: &str,
    session: &str,
) -> Result<String, CliError> {
    let workspace =
        preferred_workspace_source(root, name, session)?.unwrap_or_else(|| "(unknown)".to_owned());
    Ok(format!(
        " {} {}",
        styled(color, ANSI_BOLD_BLUE, "Workspace:"),
        styled(color, ANSI_CYAN, &workspace)
    ))
}

pub(crate) fn agent_repl_prompt(color: bool, name: &str, session: &str) -> String {
    format!(
        "{} {}{} ",
        styled(color, ANSI_DIM, "ctx agent"),
        styled(color, ANSI_BOLD_CYAN, &format!("{name}/{session}")),
        styled(color, ANSI_GREEN, " ❯")
    )
}

pub(crate) fn agent_repl_model_summary(
    color: bool,
    root: &Path,
    name: &str,
) -> Result<String, CliError> {
    let model = read_agent_model_for_context(&agent_control_dir(root, name), "agent")?;
    let model_text = styled(color, ANSI_CYAN, &model);
    if !matches!(model.as_str(), "main" | "helper") {
        return Ok(model_text);
    }
    Ok(match read_model_alias_target(root, &model) {
        Ok(target) => {
            let missing = if model_alias_target_exists(root, &target) {
                ""
            } else {
                " (missing)"
            };
            format!(
                "{} -> {}{}",
                model_text,
                styled(color, ANSI_DIM, &target),
                styled(
                    color,
                    if missing.is_empty() {
                        ANSI_GREEN
                    } else {
                        ANSI_RED
                    },
                    missing
                )
            )
        }
        Err(_error) => format!(
            "{} {}",
            model_text,
            styled(color, ANSI_RED, "(missing alias)")
        ),
    })
}

pub(crate) fn model_alias_target_exists(root: &Path, target: &str) -> bool {
    let Some(relative) = target.strip_prefix("/ctx/model/") else {
        return false;
    };
    if relative
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return false;
    }
    fs::symlink_metadata(root.join("model").join(relative))
        .is_ok_and(|metadata| !metadata.file_type().is_symlink())
}

pub(crate) fn agent_repl_editor_config() -> rustyline::Config {
    rustyline::Config::builder().enable_signals(true).build()
}

pub(crate) fn agent_repl_should_exit_on_readline_error(
    error: &rustyline::error::ReadlineError,
) -> bool {
    matches!(
        error,
        rustyline::error::ReadlineError::Interrupted
            | rustyline::error::ReadlineError::Signal(rustyline::error::Signal::Interrupt)
            | rustyline::error::ReadlineError::Eof
    )
}

pub(crate) struct AgentInterruptGuard {
    pub(crate) interrupted: Arc<AtomicBool>,
    pub(crate) signal_id: signal_hook::SigId,
}

impl AgentInterruptGuard {
    pub(crate) fn new() -> Result<Self, CliError> {
        let interrupted = Arc::new(AtomicBool::new(false));
        let signal_id =
            signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&interrupted))
                .map_err(|error| {
                    CliError::unavailable(format!("cannot register SIGINT handler: {error}"))
                })?;
        Ok(Self {
            interrupted,
            signal_id,
        })
    }

    pub(crate) fn interrupted_flag(&self) -> &AtomicBool {
        &self.interrupted
    }
}

impl Drop for AgentInterruptGuard {
    fn drop(&mut self) {
        signal_hook::low_level::unregister(self.signal_id);
    }
}

pub(crate) fn agent_repl_command(
    root: &Path,
    name: &str,
    session: &mut String,
    line: &str,
    raw: bool,
    debug: &mut AgentDebugState,
) -> Result<Option<ExitCode>, CliError> {
    let code = match line {
        "/help" => {
            print_agent_repl_banner(root, name, session)?;
            ExitCode::SUCCESS
        }
        command if command == "/new" || command.starts_with("/new ") => {
            agent_repl_new_session(root, name, session, command)?;
            ExitCode::SUCCESS
        }
        "/resume" => agent_resume(root, name, Some(session.as_str()), raw)?,
        "/history" => {
            history(root, name, Some(session.as_str()))?;
            ExitCode::SUCCESS
        }
        "/output" => {
            latest(root, name, Some(session.as_str()))?;
            ExitCode::SUCCESS
        }
        "/pack" => {
            agent_pack(root, name, Some(session.as_str()))?;
            ExitCode::SUCCESS
        }
        "/tools" => {
            agent_tools(root, name)?;
            ExitCode::SUCCESS
        }
        "/workspace" => {
            print_terminal_line(&agent_repl_workspace_line(
                color_enabled(),
                root,
                name,
                session,
            )?)?;
            ExitCode::SUCCESS
        }
        "/children" => {
            agent_children(root, name, Some(session.as_str()))?;
            ExitCode::SUCCESS
        }
        "/cancel" => agent_cancel(root, name, Some(session.as_str()), None, raw)?,
        "/debug" => {
            debug.enabled = !debug.enabled;
            write_error(if debug.enabled {
                "debug on"
            } else {
                "debug off"
            })
            .map_err(|error| CliError::unavailable(format!("stderr write failed: {error}")))?;
            ExitCode::SUCCESS
        }
        "/status" => {
            agent_status(root, name)?;
            ExitCode::SUCCESS
        }
        "/clear" => {
            clear_terminal_screen()?;
            ExitCode::SUCCESS
        }
        command if command.starts_with('/') => {
            write_error(&agent_repl_unknown_command_line(command))
                .map_err(|error| CliError::unavailable(format!("stderr write failed: {error}")))?;
            ExitCode::SUCCESS
        }
        _ => return Ok(None),
    };
    Ok(Some(code))
}

pub(crate) fn agent_repl_new_session(
    root: &Path,
    name: &str,
    current: &mut String,
    command: &str,
) -> Result<(), CliError> {
    let next = agent_repl_new_session_name(command)?;
    let session_root = ctx_home(root)?.join("agent").join(name).join("session");
    ensure_durable_session_layout(
        &session_root,
        &next,
        &agent_cwd(root, name)?,
        None,
        SocketSessionScope::Private,
    )
    .map_err(|error| {
        CliError::unavailable(format!("cannot prepare agent session {next}: {error:?}"))
    })?;
    if let Some(workspace) = preferred_workspace_source(root, name, current)? {
        write_agent_control_plain(
            &session_root.join(&next).join("workspace"),
            &format!("{workspace}\n"),
        )?;
    }
    *current = next;
    print_agent_repl_banner(root, name, current)
}

pub(crate) fn agent_repl_new_session_name(command: &str) -> Result<String, CliError> {
    let mut args = command.split_whitespace();
    let Some("/new") = args.next() else {
        return Err(CliError::usage(format!("unknown repl command: {command}")));
    };
    let session = match args.next() {
        Some(session) => session.to_owned(),
        None => request_id()?,
    };
    if args.next().is_some() {
        return Err(CliError::usage("/new accepts at most one session name"));
    }
    require_cli_name("session name", &session)?;
    Ok(session)
}

pub(crate) fn agent_repl_unknown_command_line(command: &str) -> String {
    format!("ctx: unknown repl command: {}", terminal_safe_text(command))
}

pub(crate) fn clear_terminal_screen() -> Result<(), CliError> {
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(b"\x1b[2J\x1b[H")
        .and_then(|()| stdout.flush())
        .map_err(|error| CliError::unavailable(format!("stdout write failed: {error}")))
}

#[derive(Default)]
pub(crate) struct AgentDebugState {
    pub(crate) enabled: bool,
    pub(crate) previous_tools: Option<Vec<String>>,
}

impl AgentDebugState {
    pub(crate) fn report_tools(&mut self, root: &Path, name: &str) -> Result<(), CliError> {
        let tools = agent_native_tool_names(root, name)?;
        let line = format_debug_tool_line(self.previous_tools.as_deref(), &tools);
        self.previous_tools = Some(tools);
        write_error(&line)
            .map_err(|error| CliError::unavailable(format!("stderr write failed: {error}")))
    }
}

pub(crate) fn format_debug_tool_line(previous: Option<&[String]>, current: &[String]) -> String {
    let current_text = current.join(" ");
    let Some(previous) = previous else {
        return format!("[debug tools] = {current_text}");
    };
    let added = current
        .iter()
        .filter(|tool| !previous.contains(tool))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let removed = previous
        .iter()
        .filter(|tool| !current.contains(tool))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    if added.is_empty() && removed.is_empty() {
        return format!("[debug tools] = {current_text}");
    }
    format!("[debug tools] +{added} -{removed} = {current_text}")
}
