fn agent_send(
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
        current_workspace_source().as_deref(),
        input,
        debug,
    );
    stream_agent_socket_request(&agent_chat_request_socket(root, name)?, &request, raw)
}

#[derive(Clone, Copy)]
struct AgentInteractiveSend<'a> {
    session: Option<&'a str>,
    input: &'a str,
    raw: bool,
    run_id: &'a str,
    interrupt: Option<&'a AgentInterruptGuard>,
    debug: bool,
}

const AGENT_REPL_COMMANDS: &str =
    "/help /resume /history /output /pack /tools /children /cancel /debug /status /clear /exit";

fn agent_send_interactive_with_run_id(
    root: &Path,
    name: &str,
    send: AgentInteractiveSend<'_>,
) -> Result<ExitCode, CliError> {
    let session = agent_session_name(root, name, send.session)?;
    let request = agent_send_request_json(
        send.run_id,
        &session,
        &agent_cwd(root, name)?,
        current_workspace_source().as_deref(),
        send.input,
        send.debug,
    );
    let cancel_request = format!("{{\"op\":\"cancel\",\"id\":{}}}\n", json_string(send.run_id));
    stream_agent_socket_request_streaming_interruptible(
        &agent_chat_request_socket(root, name)?,
        &request,
        send.raw,
        send.interrupt
            .map(|guard| (guard, cancel_request.as_str(), send.run_id)),
    )
}

fn agent_send_request_json(
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

fn current_workspace_source() -> Option<String> {
    env::current_dir()
        .ok()
        .filter(|path| path.is_absolute())
        .map(|path| path.display().to_string())
}

fn agent_resume(
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

fn agent_cancel(
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

fn agent_chat_request_socket(root: &Path, name: &str) -> Result<PathBuf, CliError> {
    let visible_socket = agent_socket_path(root, name)?;
    let runtime_socket = agent_chat_runtime_socket(root, name)?;
    if terminal_socket_exists(&runtime_socket) {
        return Ok(runtime_socket);
    }
    Ok(visible_socket)
}

fn agent_repl(
    root: &Path,
    name: &str,
    session: Option<&str>,
    raw: bool,
) -> Result<ExitCode, CliError> {
    let session = agent_session_name(root, name, session)?;
    let mut debug = AgentDebugState::default();
    if io::stdin().is_terminal() {
        print_agent_repl_banner(root, name, &session)?;
    }

    if io::stdin().is_terminal() {
        let mut editor =
            rustyline::DefaultEditor::with_config(agent_repl_editor_config()).map_err(|error| {
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
            if let Some(code) = agent_repl_command(root, name, &session, &line, raw, &mut debug)? {
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
            let code = agent_send_interactive_with_run_id(root, name, AgentInteractiveSend {
                session: Some(&session),
                input: &line,
                raw,
                run_id: &run_id,
                interrupt: Some(&interrupt),
                debug: debug.enabled,
            })?;
            if code != ExitCode::SUCCESS {
                return Ok(code);
            }
        }
    }

    let input = read_agent_repl_stdin_limited(io::stdin(), MAX_AGENT_REPL_STDIN_BYTES)
        .map_err(|error| CliError::unavailable(format!("cannot read stdin: {error}")))?;
    for line in input.lines() {
        if line.is_empty() {
            continue;
        }
        if matches!(line, "/exit" | "/quit") {
            break;
        }
        if agent_repl_command(root, name, &session, line, raw, &mut debug)?.is_none() {
            if debug.enabled {
                debug.report_tools(root, name)?;
            }
            let _code = agent_send(root, name, Some(&session), line, raw, debug.enabled)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn read_agent_repl_stdin_limited(
    reader: impl Read,
    max_bytes: usize,
) -> io::Result<String> {
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
            "agent stdin exceeds input limit",
        ));
    }
    Ok(input)
}
