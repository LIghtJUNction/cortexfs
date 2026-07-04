fn print_agent_repl_banner(root: &Path, name: &str, session: &str) -> Result<(), CliError> {
    let color = color_enabled();
    let model_summary = agent_repl_model_summary(color, root, name)?;
    let lines = [
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
        format!(
            " {} {}",
            styled(color, ANSI_BOLD_BLUE, "Commands:"),
            styled(color, ANSI_DIM, AGENT_REPL_COMMANDS)
        ),
    ];
    for line in lines {
        write_error(&line)
            .map_err(|error| CliError::unavailable(format!("stderr write failed: {error}")))?;
    }
    Ok(())
}

fn agent_repl_prompt(color: bool, name: &str, session: &str) -> String {
    format!(
        "{} {}{} ",
        styled(color, ANSI_DIM, "ctx agent"),
        styled(color, ANSI_BOLD_CYAN, &format!("{name}/{session}")),
        styled(color, ANSI_GREEN, " ❯")
    )
}

fn agent_repl_model_summary(color: bool, root: &Path, name: &str) -> Result<String, CliError> {
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
        Err(_error) => format!("{} {}", model_text, styled(color, ANSI_RED, "(missing alias)")),
    })
}

fn model_alias_target_exists(root: &Path, target: &str) -> bool {
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

fn read_model_alias_target(root: &Path, model: &str) -> io::Result<String> {
    let model_dir = open_agent_terminal_runtime_dir(&root.join("model"))?;
    let target = nix::fcntl::readlinkat(&model_dir, model).map_err(io::Error::from)?;
    Ok(target.to_string_lossy().into_owned())
}

fn agent_repl_editor_config() -> rustyline::Config {
    rustyline::Config::builder().enable_signals(true).build()
}

fn agent_repl_should_exit_on_readline_error(error: &rustyline::error::ReadlineError) -> bool {
    matches!(
        error,
        rustyline::error::ReadlineError::Interrupted
            | rustyline::error::ReadlineError::Signal(rustyline::error::Signal::Interrupt)
            | rustyline::error::ReadlineError::Eof
    )
}

struct AgentInterruptGuard {
    interrupted: Arc<AtomicBool>,
    signal_id: signal_hook::SigId,
}

impl AgentInterruptGuard {
    fn new() -> Result<Self, CliError> {
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

    fn interrupted_flag(&self) -> &AtomicBool {
        &self.interrupted
    }
}

impl Drop for AgentInterruptGuard {
    fn drop(&mut self) {
        signal_hook::low_level::unregister(self.signal_id);
    }
}

fn agent_repl_command(
    root: &Path,
    name: &str,
    session: &str,
    line: &str,
    raw: bool,
    debug: &mut AgentDebugState,
) -> Result<Option<ExitCode>, CliError> {
    let code = match line {
        "/help" => {
            print_agent_repl_banner(root, name, session)?;
            ExitCode::SUCCESS
        }
        "/resume" => agent_resume(root, name, Some(session), raw)?,
        "/history" => {
            history(root, name, Some(session))?;
            ExitCode::SUCCESS
        }
        "/output" => {
            latest(root, name, Some(session))?;
            ExitCode::SUCCESS
        }
        "/pack" => {
            agent_pack(root, name, Some(session))?;
            ExitCode::SUCCESS
        }
        "/tools" => {
            agent_tools(root, name)?;
            ExitCode::SUCCESS
        }
        "/children" => {
            agent_children(root, name, Some(session))?;
            ExitCode::SUCCESS
        }
        "/cancel" => agent_cancel(root, name, Some(session), None, raw)?,
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

fn agent_repl_unknown_command_line(command: &str) -> String {
    format!("ctx: unknown repl command: {}", terminal_safe_text(command))
}

fn clear_terminal_screen() -> Result<(), CliError> {
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(b"\x1b[2J\x1b[H")
        .and_then(|()| stdout.flush())
        .map_err(|error| CliError::unavailable(format!("stdout write failed: {error}")))
}

#[derive(Default)]
struct AgentDebugState {
    enabled: bool,
    previous_tools: Option<Vec<String>>,
}

impl AgentDebugState {
    fn report_tools(&mut self, root: &Path, name: &str) -> Result<(), CliError> {
        let tools = agent_native_tool_names(root, name)?;
        let line = format_debug_tool_line(self.previous_tools.as_deref(), &tools);
        self.previous_tools = Some(tools);
        write_error(&line)
            .map_err(|error| CliError::unavailable(format!("stderr write failed: {error}")))
    }
}

fn format_debug_tool_line(previous: Option<&[String]>, current: &[String]) -> String {
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
