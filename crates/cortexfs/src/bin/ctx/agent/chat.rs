use crate::*;

pub(crate) fn agent_send(
    root: &Path,
    name: &str,
    session: Option<&str>,
    input: &str,
    raw: bool,
    debug: bool,
) -> Result<ExitCode, CliError> {
    let session = agent_session_name(root, name, session)?;
    let request = agent_send_request_json(
        &request_id()?,
        &session,
        &agent_cwd(root, name)?,
        preferred_workspace_source(root, name, &session)?.as_deref(),
        input,
        debug,
    );
    stream_agent_socket_request(&agent_chat_request_socket(root, name)?, &request, raw)
}

#[derive(Clone, Copy)]
pub(crate) struct AgentInteractiveSend<'a> {
    pub(crate) session: Option<&'a str>,
    pub(crate) input: &'a str,
    pub(crate) raw: bool,
    pub(crate) run_id: &'a str,
    pub(crate) interrupt: Option<&'a AgentInterruptGuard>,
    pub(crate) debug: bool,
}

pub(crate) const AGENT_REPL_COMMANDS: &str = "/help /new [session] /workspace /resume /history /output /pack /tools /children /cancel /debug /status /clear /exit";

pub(crate) fn agent_send_interactive_with_run_id(
    root: &Path,
    name: &str,
    send: AgentInteractiveSend<'_>,
) -> Result<ExitCode, CliError> {
    let session = agent_session_name(root, name, send.session)?;
    let request = agent_send_request_json(
        send.run_id,
        &session,
        &agent_cwd(root, name)?,
        preferred_workspace_source(root, name, &session)?.as_deref(),
        send.input,
        send.debug,
    );
    let cancel_request = format!(
        "{{\"op\":\"cancel\",\"id\":{}}}\n",
        json_string(send.run_id)
    );
    stream_agent_socket_request_streaming_interruptible(
        &agent_chat_request_socket(root, name)?,
        &request,
        send.raw,
        send.interrupt
            .map(|guard| (guard, cancel_request.as_str(), send.run_id)),
    )
}

pub(crate) fn agent_send_request_json(
    run_id: &str,
    session: &str,
    cwd: &str,
    workspace: Option<&str>,
    input: &str,
    debug: bool,
) -> String {
    let workspace = workspace.map_or_else(String::new, |workspace| {
        format!(",\"workspace\":{}", json_string(workspace))
    });
    if debug {
        return format!(
            "{{\"op\":\"send\",\"id\":{},\"session\":{},\"scope\":\"private\",\"cwd\":{}{},\"input\":{},\"debug\":true}}\n",
            json_string(run_id),
            json_string(session),
            json_string(cwd),
            workspace,
            json_string(input)
        );
    }
    format!(
        "{{\"op\":\"send\",\"id\":{},\"session\":{},\"scope\":\"private\",\"cwd\":{}{},\"input\":{}}}\n",
        json_string(run_id),
        json_string(session),
        json_string(cwd),
        workspace,
        json_string(input)
    )
}

pub(crate) fn current_workspace_source() -> Option<String> {
    env::current_dir()
        .ok()
        .filter(|path| path.is_absolute())
        .map(|path| path.display().to_string())
}

pub(crate) fn preferred_workspace_source(
    root: &Path,
    name: &str,
    session: &str,
) -> Result<Option<String>, CliError> {
    let workspace = agent_session_workspace_source(root, name, session)?;
    Ok(workspace.or_else(current_workspace_source))
}

pub(crate) fn agent_session_workspace_source(
    root: &Path,
    name: &str,
    session: &str,
) -> Result<Option<String>, CliError> {
    let path = agent_session_dir(root, name, Some(session))?.join("workspace");
    let Some(workspace) = read_optional_trimmed(&path)? else {
        return Ok(None);
    };
    Ok((!workspace.bytes().any(|byte| byte.is_ascii_control())
        && Path::new(&workspace).is_absolute()
        && Path::new(&workspace).components().all(|component| {
            matches!(
                component,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        }))
    .then_some(workspace))
}

pub(crate) fn agent_resume(
    root: &Path,
    name: &str,
    session: Option<&str>,
    raw: bool,
) -> Result<ExitCode, CliError> {
    let session = agent_session_name(root, name, session)?;
    let request = format!(
        "{{\"op\":\"resume\",\"session\":{}}}\n",
        json_string(&session)
    );
    stream_agent_socket_request(&agent_chat_request_socket(root, name)?, &request, raw)
}

pub(crate) fn agent_cancel(
    root: &Path,
    name: &str,
    session: Option<&str>,
    run: Option<&str>,
    raw: bool,
) -> Result<ExitCode, CliError> {
    let session = agent_session_name(root, name, session)?;
    let run = match run {
        Some(run) => run.to_owned(),
        None => latest_run_id(root, name, &session)?,
    };
    require_cli_name("run id", &run)?;
    let request = format!("{{\"op\":\"cancel\",\"id\":{}}}\n", json_string(&run));
    stream_agent_socket_request(&agent_chat_request_socket(root, name)?, &request, raw)
}

pub(crate) fn agent_chat_request_socket(root: &Path, name: &str) -> Result<PathBuf, CliError> {
    let runtime_socket = agent_chat_runtime_socket(root, name)?;
    if terminal_socket_exists(&runtime_socket) {
        return Ok(runtime_socket);
    }
    let visible_socket = agent_socket_path(root, name)?;
    if terminal_socket_exists(&visible_socket) {
        return Ok(visible_socket);
    }
    Ok(visible_socket)
}

pub(crate) fn agent_repl(
    root: &Path,
    name: &str,
    session: Option<&str>,
    raw: bool,
) -> Result<ExitCode, CliError> {
    let mut session = agent_session_name(root, name, session)?;
    let mut debug = AgentDebugState::default();
    if io::stdin().is_terminal() {
        print_agent_repl_banner(root, name, &session)?;
    }

    if io::stdin().is_terminal() {
        let mut editor = rustyline::DefaultEditor::with_config(agent_repl_editor_config())
            .map_err(|error| {
                CliError::unavailable(format!("cannot initialize line editor: {error}"))
            })?;
        let color = color_enabled();
        loop {
            let prompt = agent_repl_prompt(color, name, &session);
            let line = match editor.readline(&prompt) {
                Ok(line) => line,
                Err(error) if agent_repl_should_exit_on_readline_error(&error) => {
                    return Ok(ExitCode::SUCCESS);
                }
                Err(error) => {
                    return Err(CliError::unavailable(format!(
                        "cannot read interactive input: {error}"
                    )));
                }
            };
            if line.is_empty() {
                continue;
            }
            let _ignored = editor.add_history_entry(line.as_str());
            if matches!(line.as_str(), "/exit" | "/quit") {
                return Ok(ExitCode::SUCCESS);
            }
            if let Some(code) =
                agent_repl_command(root, name, &mut session, &line, raw, &mut debug)?
            {
                if code != ExitCode::SUCCESS {
                    return Ok(code);
                }
                continue;
            }
            if !raw {
                print_terminal_text("\n")?;
            }
            if debug.enabled {
                debug.report_tools(root, name)?;
            }
            let run_id = request_id()?;
            let interrupt = AgentInterruptGuard::new()?;
            let code = agent_send_interactive_with_run_id(
                root,
                name,
                AgentInteractiveSend {
                    session: Some(&session),
                    input: &line,
                    raw,
                    run_id: &run_id,
                    interrupt: Some(&interrupt),
                    debug: debug.enabled,
                },
            )?;
            if code != ExitCode::SUCCESS {
                return Ok(code);
            }
        }
    }

    let input = read_limited_input_text(
        io::stdin(),
        MAX_AGENT_REPL_STDIN_BYTES,
        "agent stdin exceeds input limit",
    )
    .map_err(|error| CliError::unavailable(format!("cannot read stdin: {error}")))?;
    for line in input.lines() {
        if line.is_empty() {
            continue;
        }
        if matches!(line, "/exit" | "/quit") {
            break;
        }
        if agent_repl_command(root, name, &mut session, line, raw, &mut debug)?.is_none() {
            if debug.enabled {
                debug.report_tools(root, name)?;
            }
            let _code = agent_send(root, name, Some(&session), line, raw, debug.enabled)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}
