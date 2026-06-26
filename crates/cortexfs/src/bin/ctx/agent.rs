#[derive(Debug, Eq, PartialEq)]
enum AgentArgs {
    New(AgentNewArgs),
    Start(AgentStartArgs),
    Stop { name: String },
    Status { name: String },
    Ps,
    Send {
        name: String,
        session: Option<String>,
        input: String,
        raw: bool,
    },
    Repl {
        name: String,
        session: Option<String>,
        raw: bool,
    },
    Resume {
        name: String,
        session: Option<String>,
        raw: bool,
    },
    History {
        name: String,
        session: Option<String>,
    },
    Output {
        name: String,
        session: Option<String>,
    },
    Pack {
        name: String,
        session: Option<String>,
    },
    Prompt {
        name: String,
    },
    Tools {
        name: String,
    },
    Children {
        name: String,
        session: Option<String>,
    },
    Cancel {
        name: String,
        session: Option<String>,
        run: Option<String>,
        raw: bool,
    },
    Watch {
        name: String,
        session: Option<String>,
    },
    Attach {
        name: String,
        session: Option<String>,
    },
}

#[derive(Debug, Eq, PartialEq)]
struct AgentNewArgs {
    name: String,
    temporary: bool,
    label: Option<String>,
    models: Vec<String>,
    tools: Vec<String>,
    shared: Vec<AgentShared>,
    mounts: Vec<AgentMount>,
}

#[derive(Debug, Eq, PartialEq)]
struct AgentStartArgs {
    name: String,
    session: String,
    cwd: String,
    default_workspace: bool,
    mounts: Vec<AgentMount>,
}

#[derive(Debug, Eq, PartialEq)]
struct AgentShared {
    name: String,
    access: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AgentMount {
    source: String,
    target: String,
    mode: String,
}

const AGENT_SANDBOX_HOME: &str = "/home/agent";

fn agent_command(root: &Path, args: &AgentArgs) -> Result<ExitCode, CliError> {
    match *args {
        AgentArgs::New(ref args) => agent_lifecycle_tool(
            root,
            "agent.create",
            &agent_new_request_json(args)?,
        ),
        AgentArgs::Start(ref args) => agent_start(root, args),
        AgentArgs::Stop { ref name } => {
            require_cli_name("agent name", name)?;
            agent_lifecycle_tool(root, "agent.stop", &agent_name_request_json(name))
        }
        AgentArgs::Status { ref name } => {
            require_cli_name("agent name", name)?;
            success(cat_path(
                &root.join("agent").join(format!("{name}.d")).join("status"),
            ))
        }
        AgentArgs::Ps => success(agent_ps(root)),
        AgentArgs::Send {
            ref name,
            ref session,
            ref input,
            raw,
        } => agent_send(root, name, session.as_deref(), input, raw),
        AgentArgs::Repl {
            ref name,
            ref session,
            raw,
        } => agent_repl(root, name, session.as_deref(), raw),
        AgentArgs::Resume {
            ref name,
            ref session,
            raw,
        } => agent_resume(root, name, session.as_deref(), raw),
        AgentArgs::History {
            ref name,
            ref session,
        } => success(history(root, name, session.as_deref())),
        AgentArgs::Output {
            ref name,
            ref session,
        } => success(latest(root, name, session.as_deref())),
        AgentArgs::Pack {
            ref name,
            ref session,
        } => success(agent_pack(root, name, session.as_deref())),
        AgentArgs::Prompt { ref name } => success(agent_prompt(root, name)),
        AgentArgs::Tools { ref name } => success(agent_tools(root, name)),
        AgentArgs::Children {
            ref name,
            ref session,
        } => success(agent_children(root, name, session.as_deref())),
        AgentArgs::Cancel {
            ref name,
            ref session,
            ref run,
            raw,
        } => agent_cancel(root, name, session.as_deref(), run.as_deref(), raw),
        AgentArgs::Watch {
            ref name,
            ref session,
        } => agent_terminal(root, name, session.as_deref(), false),
        AgentArgs::Attach {
            ref name,
            ref session,
        } => agent_terminal(root, name, session.as_deref(), true),
    }
}

fn agent_start(root: &Path, args: &AgentStartArgs) -> Result<ExitCode, CliError> {
    require_cli_name("agent name", &args.name)?;
    require_session_name(&args.session)?;
    require_sandbox_cwd(&args.cwd)?;
    let cli_mounts = agent_start_mounts(args)?;
    for mount in &cli_mounts {
        require_agent_mount(mount)?;
    }
    let view = derive_agent_runtime_view(root, &args.name).map_err(|error| {
        CliError::unavailable(format!(
            "cannot derive agent runtime view for {}: {error:?}",
            args.name
        ))
    })?;
    let visible_socket = agent_terminal_socket(root, &args.name, &args.session)?;
    let socket = agent_runtime_socket(root, &args.name, &args.session)?;
    ensure_agent_terminal_socket(&visible_socket, &socket)?;
    let unit = agent_terminal_unit(&args.name, &args.session);
    reset_agent_terminal_unit(&unit);
    let command = agent_start_systemd_command(root, args, &cli_mounts, &view, &socket, &unit);
    let output = ProcessCommand::new(&command.program)
        .args(&command.args)
        .output()
        .map_err(|error| CliError::unavailable(format!("cannot start systemd-run: {error}")))?;
    if !output.status.success() {
        let diagnostics = systemd_run_diagnostics(&output);
        return Err(CliError::unavailable(format!(
            "agent terminal service failed to start with {}{diagnostics}",
            output.status
        )));
    }
    wait_for_agent_terminal_socket(&socket)?;
    let invocation = systemd_run_invocation_id(&output);
    let uid = current_uid_for_ctx(root)?;
    for line in agent_start_status_lines(
        color_enabled(),
        &args.name,
        &args.session,
        &unit,
        invocation.as_deref(),
        &args.cwd,
        &visible_socket,
        &socket,
        &uid,
    ) {
        print_line(&line)?;
    }
    Ok(ExitCode::SUCCESS)
}

#[expect(
    clippy::too_many_arguments,
    reason = "status output mirrors systemctl's flat field list"
)]
fn agent_start_status_lines(
    color: bool,
    agent: &str,
    session: &str,
    unit: &str,
    invocation: Option<&str>,
    cwd: &str,
    visible_socket: &Path,
    runtime_socket: &Path,
    uid: &str,
) -> Vec<String> {
    let service = format!("{unit}.service");
    let loaded_path = format!("/run/user/{uid}/systemd/transient/{service}");
    let loaded = styled(color, ANSI_GREEN, "loaded");
    let mut lines = vec![
        format!(
            "{} {} - {}",
            styled(color, ANSI_BOLD_CYAN, "●"),
            styled(color, ANSI_BOLD_CYAN, &service),
            styled(color, ANSI_CYAN, "CortexFS agent terminal")
        ),
        format!(
            "     {} {} ({loaded_path}; transient)",
            styled(color, ANSI_BOLD_BLUE, "Loaded:"),
            loaded
        ),
        format!(
            "     {} {}",
            styled(color, ANSI_BOLD_BLUE, "Active:"),
            styled(color, ANSI_GREEN, "active (running)")
        ),
    ];
    if let Some(invocation) = invocation {
        lines.push(format!(
            " {} {}",
            styled(color, ANSI_BOLD_BLUE, "Invocation:"),
            styled(color, ANSI_DIM, invocation)
        ));
    }
    lines.extend([
        format!(
            "      {} {}",
            styled(color, ANSI_BOLD_BLUE, "Agent:"),
            styled(color, ANSI_CYAN, agent)
        ),
        format!(
            "    {} {}",
            styled(color, ANSI_BOLD_BLUE, "Session:"),
            styled(color, ANSI_CYAN, session)
        ),
        format!(
            "        {} {}",
            styled(color, ANSI_BOLD_BLUE, "CWD:"),
            styled(color, ANSI_CYAN, cwd)
        ),
        format!(
            "     {} {}",
            styled(color, ANSI_BOLD_BLUE, "Socket:"),
            styled(color, ANSI_CYAN, &visible_socket.display().to_string())
        ),
        format!(
            " {} {}",
            styled(color, ANSI_BOLD_BLUE, "Runtime Socket:"),
            styled(color, ANSI_DIM, &runtime_socket.display().to_string())
        ),
    ]);
    lines
}

fn systemd_run_invocation_id(output: &std::process::Output) -> Option<String> {
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text.lines().find_map(|line| {
        line.rsplit_once("invocation ID: ")
            .map(|(_prefix, value)| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

fn systemd_run_diagnostics(output: &std::process::Output) -> String {
    let mut diagnostics = String::new();
    for bytes in [&output.stderr, &output.stdout] {
        let text = String::from_utf8_lossy(bytes);
        let text = text.trim();
        if !text.is_empty() {
            if diagnostics.is_empty() {
                diagnostics.push_str(": ");
            } else {
                diagnostics.push('\n');
            }
            diagnostics.push_str(text);
        }
    }
    diagnostics
}

fn reset_agent_terminal_unit(unit: &str) {
    let service = format!("{unit}.service");
    let _ignored = ProcessCommand::new("systemctl")
        .args(["--user", "stop", &service])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ignored = ProcessCommand::new("systemctl")
        .args(["--user", "reset-failed", &service])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn agent_send(
    root: &Path,
    name: &str,
    session: Option<&str>,
    input: &str,
    raw: bool,
) -> Result<ExitCode, CliError> {
    let session = agent_session_name(root, name, session)?;
    let request = format!(
        "{{\"op\":\"send\",\"id\":{},\"session\":{},\"scope\":\"private\",\"cwd\":{},\"input\":{}}}\n",
        json_string(&request_id()?),
        json_string(&session),
        json_string(&agent_cwd(root, name)?),
        json_string(input)
    );
    stream_agent_socket_request(&agent_socket_path(root, name)?, &request, raw)
}

#[derive(Clone, Copy)]
struct AgentBufferedSend<'a> {
    session: Option<&'a str>,
    input: &'a str,
    raw: bool,
    run_id: &'a str,
    interrupt: Option<&'a AgentInterruptGuard>,
}

fn agent_send_buffered_with_run_id(
    root: &Path,
    name: &str,
    send: AgentBufferedSend<'_>,
) -> Result<ExitCode, CliError> {
    let session = agent_session_name(root, name, send.session)?;
    let request = format!(
        "{{\"op\":\"send\",\"id\":{},\"session\":{},\"scope\":\"private\",\"cwd\":{},\"input\":{}}}\n",
        json_string(send.run_id),
        json_string(&session),
        json_string(&agent_cwd(root, name)?),
        json_string(send.input)
    );
    let cancel_request = format!("{{\"op\":\"cancel\",\"id\":{}}}\n", json_string(send.run_id));
    stream_agent_socket_request_buffered_interruptible(
        &agent_socket_path(root, name)?,
        &request,
        send.raw,
        send.interrupt
            .map(|guard| (guard, cancel_request.as_str(), send.run_id)),
    )
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
    stream_agent_socket_request(&agent_socket_path(root, name)?, &request, raw)
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
    stream_agent_socket_request(&agent_socket_path(root, name)?, &request, raw)
}

fn agent_repl(
    root: &Path,
    name: &str,
    session: Option<&str>,
    raw: bool,
) -> Result<ExitCode, CliError> {
    let session = agent_session_name(root, name, session)?;
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
            if let Some(code) = agent_repl_command(root, name, &session, &line, raw)? {
                if code != ExitCode::SUCCESS {
                    return Ok(code);
                }
                continue;
            }
            if !raw {
                print_terminal_text("\n")?;
            }
            let run_id = request_id()?;
            let interrupt = AgentInterruptGuard::new()?;
            let code = agent_send_buffered_with_run_id(root, name, AgentBufferedSend {
                session: Some(&session),
                input: &line,
                raw,
                run_id: &run_id,
                interrupt: Some(&interrupt),
            })?;
            if code != ExitCode::SUCCESS {
                return Ok(code);
            }
        }
    }

    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| CliError::unavailable(format!("cannot read stdin: {error}")))?;
    for line in input.lines() {
        if line.is_empty() {
            continue;
        }
        if matches!(line, "/exit" | "/quit") {
            break;
        }
        if agent_repl_command(root, name, &session, line, raw)?.is_none() {
            let _code = agent_send(root, name, Some(&session), line, raw)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn print_agent_repl_banner(root: &Path, name: &str, session: &str) -> Result<(), CliError> {
    let color = color_enabled();
    let lines = [
        format!(
            "{} {}/{} - {}",
            styled(color, ANSI_BOLD_CYAN, "●"),
            styled(color, ANSI_BOLD_CYAN, name),
            styled(color, ANSI_CYAN, session),
            styled(color, ANSI_CYAN, "CortexFS agent chat")
        ),
        format!(
            "    {} {}",
            styled(color, ANSI_BOLD_BLUE, "Model:"),
            agent_repl_model_summary(color, root, name)
        ),
        format!(
            " {} {}",
            styled(color, ANSI_BOLD_BLUE, "Commands:"),
            styled(
                color,
                ANSI_DIM,
                "/resume /history /output /pack /tools /children /cancel /status /exit"
            )
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
        "{}{} ",
        styled(color, ANSI_BOLD_CYAN, &format!("{name}/{session}")),
        styled(color, ANSI_GREEN, " ❯")
    )
}

fn agent_repl_model_summary(color: bool, root: &Path, name: &str) -> String {
    let model = fs::read_to_string(root.join("agent").join(format!("{name}.d")).join("model"))
        .ok()
        .map(|content| content.trim().to_owned())
        .filter(|content| !content.is_empty())
        .unwrap_or_else(|| "main".to_owned());
    let model_text = styled(color, ANSI_CYAN, &model);
    if !matches!(model.as_str(), "main" | "helper") {
        return model_text;
    }
    match fs::read_link(root.join("model").join(&model)) {
        Ok(target) => {
            let missing = if target.exists() { "" } else { " (missing)" };
            format!(
                "{} -> {}{}",
                model_text,
                styled(color, ANSI_DIM, &target.display().to_string()),
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
    }
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
) -> Result<Option<ExitCode>, CliError> {
    let code = match line {
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
        "/status" => {
            agent_status(root, name)?;
            ExitCode::SUCCESS
        }
        command if command.starts_with('/') => {
            write_error(&format!("ctx: unknown repl command: {command}"))
                .map_err(|error| CliError::unavailable(format!("stderr write failed: {error}")))?;
            ExitCode::SUCCESS
        }
        _ => return Ok(None),
    };
    Ok(Some(code))
}

fn agent_status(root: &Path, name: &str) -> Result<(), CliError> {
    require_cli_name("agent name", name)?;
    cat_path(&root.join("agent").join(format!("{name}.d")).join("status"))
}

fn agent_pack(root: &Path, name: &str, session: Option<&str>) -> Result<(), CliError> {
    let session_dir = agent_session_dir(root, name, session)?;
    let context = session_dir.join("context");
    for file in ["pack.md", "pack.json", "summary.md"] {
        let path = context.join(file);
        if path.is_file() {
            return cat_path(&path);
        }
    }
    Err(CliError::unavailable(format!(
        "missing context pack: {}",
        context.join("pack.md").display()
    )))
}

fn agent_prompt(root: &Path, name: &str) -> Result<(), CliError> {
    let prompt = build_agent_system_prompt(root, name, &current_time_unix().to_string())?;
    print_terminal_text(&prompt)
}

fn build_agent_system_prompt(
    root: &Path,
    name: &str,
    current_time_unix: &str,
) -> Result<String, CliError> {
    require_cli_name("agent name", name)?;
    let control = root.join("agent").join(format!("{name}.d"));
    let agent_system = read_optional_trimmed(&control.join("system.md"))?.unwrap_or_default();
    let template = read_optional_trimmed(&control.join("prompt.template.md"))?
        .unwrap_or_else(|| DEFAULT_AGENT_PROMPT_TEMPLATE.to_owned());
    Ok(render_agent_system_prompt(
        name,
        &agent_system,
        &AgentPromptContext {
            template,
            rules: collect_agent_rules(),
            skills: collect_skill_metadata(skill_metadata_budget_from_env()),
            tool_injection: "(no repo structure, search result, or file content injected)"
                .to_owned(),
            history_messages: "(no historical messages injected)".to_owned(),
            current_time_unix: current_time_unix.to_owned(),
        },
    ))
}

fn agent_children(root: &Path, name: &str, session: Option<&str>) -> Result<(), CliError> {
    let child_root = agent_session_dir(root, name, session)?.join("context").join("child");
    if !child_root.is_dir() {
        return Ok(());
    }
    for child in read_dir_names(&child_root)? {
        let dir = child_root.join(&child);
        if !dir.is_dir() {
            continue;
        }
        let status = read_optional_trimmed(&dir.join("status"))?.unwrap_or_else(|| "unknown".to_owned());
        let agent = read_optional_trimmed(&dir.join("agent"))?.unwrap_or_else(|| "agent?".to_owned());
        print_line(&format!("{child}\t{status}\t{agent}"))?;
    }
    Ok(())
}

fn agent_tools(root: &Path, name: &str) -> Result<(), CliError> {
    require_cli_name("agent name", name)?;
    let mut paths = Vec::new();
    paths.extend(ctx_tool_path(root)?.dirs().iter().map(PathBuf::from));
    let agent_path = root.join("agent").join(format!("{name}.d")).join("path");
    if let Ok(content) = fs::read_to_string(agent_path) {
        paths.extend(content.lines().map(PathBuf::from));
    }
    paths.sort();
    paths.dedup();
    for path in paths {
        if !path.is_dir() {
            continue;
        }
        for tool in read_dir_names(&path)? {
            let tool_path = path.join(&tool);
            if !is_executable_file(&tool_path) || is_control_or_socket_name(&tool) {
                continue;
            }
            let status = read_optional_trimmed(&tool_path.with_file_name(format!("{tool}.d")).join("status"))?
                .unwrap_or_else(|| "unknown".to_owned());
            print_line(&format!("{tool}\t{}\t{status}", tool_path.display()))?;
        }
    }
    Ok(())
}

fn is_control_or_socket_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("sock") || ext.eq_ignore_ascii_case("d"))
}

fn agent_cwd(root: &Path, name: &str) -> Result<String, CliError> {
    let path = root.join("agent").join(format!("{name}.d")).join("cwd");
    Ok(read_optional_trimmed(&path)?.unwrap_or_else(|| "/workspace".to_owned()))
}

fn latest_run_id(root: &Path, name: &str, session: &str) -> Result<String, CliError> {
    let session_dir = agent_session_dir(root, name, Some(session))?;
    if let Some(run) = read_optional_trimmed(&session_dir.join("current_run"))? {
        return Ok(run);
    }
    let events = session_dir.join("events.jsonl");
    let content = fs::read_to_string(&events)
        .map_err(|error| CliError::unavailable(format!("cannot read {}: {error}", events.display())))?;
    let mut latest = None;
    for line in content.lines() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            latest = value
                .get("run")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .or(latest);
        }
    }
    latest.ok_or_else(|| CliError::unavailable("missing run id; pass RUN explicitly"))
}

fn read_optional_trimmed(path: &Path) -> Result<Option<String>, CliError> {
    match fs::read_to_string(path) {
        Ok(value) => {
            let value = value.trim();
            if value.is_empty() {
                Ok(None)
            } else {
                Ok(Some(value.to_owned()))
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(CliError::unavailable(format!(
            "cannot read {}: {error}",
            path.display()
        ))),
    }
}

#[derive(Debug, Eq, PartialEq)]
struct AgentStartCommand {
    program: String,
    args: Vec<String>,
}

fn agent_start_systemd_command(
    root: &Path,
    args: &AgentStartArgs,
    cli_mounts: &[AgentMount],
    view: &AgentRuntimeView,
    socket: &Path,
    unit: &str,
) -> AgentStartCommand {
    let home = view.ctx_home();
    let mut command = AgentStartCommand {
        program: "systemd-run".to_owned(),
        args: vec![
            "--user".to_owned(),
            "--unit".to_owned(),
            unit.to_owned(),
            "--property".to_owned(),
            "Restart=always".to_owned(),
            "--property".to_owned(),
            "RestartSec=250ms".to_owned(),
            "/usr/bin/env".to_owned(),
            "-i".to_owned(),
            "PATH=/usr/bin:/bin".to_owned(),
            "/usr/bin/bwrap".to_owned(),
        ],
    };
    command
        .args
        .extend(agent_bwrap_args(root, args, cli_mounts, view, socket, home));
    command
}

fn agent_sandbox_env(_root: &Path, view: &AgentRuntimeView) -> Vec<(String, String)> {
    let mut env = vec![
        ("CTX_ROOT".to_owned(), view.ctx_root().display().to_string()),
        ("CTX_HOME".to_owned(), view.ctx_home().display().to_string()),
        ("CTX_AGENT".to_owned(), view.agent_name().to_owned()),
        (
            "CTX_AGENT_SUBJECT".to_owned(),
            view.policy_subject().to_owned(),
        ),
        ("HOME".to_owned(), AGENT_SANDBOX_HOME.to_owned()),
        ("USER".to_owned(), view.agent_name().to_owned()),
        ("LOGNAME".to_owned(), view.agent_name().to_owned()),
        ("SHELL".to_owned(), "/usr/bin/bash".to_owned()),
        ("TERM".to_owned(), "xterm-256color".to_owned()),
        ("LANG".to_owned(), "C.UTF-8".to_owned()),
    ];
    for env_pair in view.env() {
        let key = &env_pair.0;
        let value = &env_pair.1;
        if matches!(
            key.as_str(),
            "CTX_ROOT"
                | "CTX_HOME"
                | "CTX_AGENT"
                | "CTX_AGENT_SUBJECT"
                | "HOME"
                | "USER"
                | "LOGNAME"
                | "SHELL"
                | "TERM"
                | "LANG"
        ) {
            continue;
        }
        env.push((key.clone(), value.clone()));
    }
    env
}

fn agent_bwrap_args(
    root: &Path,
    args: &AgentStartArgs,
    cli_mounts: &[AgentMount],
    view: &AgentRuntimeView,
    socket: &Path,
    _home: &Path,
) -> Vec<String> {
    let agent_home = view.home();
    let mut bwrap = vec!["--clearenv".to_owned()];
    for (key, value) in agent_sandbox_env(root, view) {
        bwrap.extend(["--setenv".to_owned(), key, value]);
    }
    bwrap.extend([
        "--die-with-parent".to_owned(),
        "--unshare-pid".to_owned(),
        "--unshare-net".to_owned(),
        "--proc".to_owned(),
        "/proc".to_owned(),
        "--dev".to_owned(),
        "/dev".to_owned(),
        "--tmpfs".to_owned(),
        "/tmp".to_owned(),
        "--dir".to_owned(),
        "/run".to_owned(),
        "--dir".to_owned(),
        "/home".to_owned(),
        "--ro-bind".to_owned(),
        "/usr".to_owned(),
        "/usr".to_owned(),
        "--ro-bind".to_owned(),
        "/etc".to_owned(),
        "/etc".to_owned(),
        "--tmpfs".to_owned(),
        "/etc/profile.d".to_owned(),
        "--symlink".to_owned(),
        "usr/bin".to_owned(),
        "/bin".to_owned(),
        "--symlink".to_owned(),
        "usr/lib".to_owned(),
        "/lib".to_owned(),
        "--symlink".to_owned(),
        "usr/lib".to_owned(),
        "/lib64".to_owned(),
    ]);
    if let Some(runtime_dir) = socket_runtime_dir(socket) {
        bwrap.extend([
            "--bind".to_owned(),
            runtime_dir.display().to_string(),
            runtime_dir.display().to_string(),
        ]);
    }
    for mount in view.mount_table().entries() {
        bwrap.push(match mount.mode() {
            MountMode::ReadOnly => "--ro-bind".to_owned(),
            MountMode::ReadWrite => "--bind".to_owned(),
        });
        bwrap.push(mount.source().to_owned());
        let target = if mount.target() == agent_home {
            AGENT_SANDBOX_HOME.to_owned()
        } else {
            mount.target().to_owned()
        };
        bwrap.push(target);
    }
    for mount in cli_mounts {
        bwrap.push(match mount.mode.as_str() {
            "ro" => "--ro-bind".to_owned(),
            _ => "--bind".to_owned(),
        });
        bwrap.push(mount.source.clone());
        bwrap.push(mount.target.clone());
    }
    if let Some(startup_stub) = shell_startup_stub_path(socket) {
        bwrap.extend([
            "--ro-bind".to_owned(),
            startup_stub.display().to_string(),
            "/etc/profile".to_owned(),
            "--ro-bind".to_owned(),
            startup_stub.display().to_string(),
            "/etc/bash.bashrc".to_owned(),
        ]);
    }
    bwrap.extend([
        "--chdir".to_owned(),
        args.cwd.clone(),
        "/usr/bin/ctxterm".to_owned(),
        "--listen".to_owned(),
        socket.display().to_string(),
        "--no-stdio".to_owned(),
        "--".to_owned(),
        "/ctx/bin/tsh".to_owned(),
    ]);
    bwrap
}

fn agent_start_mounts(args: &AgentStartArgs) -> Result<Vec<AgentMount>, CliError> {
    let default_source = env::current_dir()
        .map_err(|error| CliError::unavailable(format!("cannot read current directory: {error}")))?;
    Ok(agent_start_mounts_with_default_source(args, &default_source))
}

fn agent_start_mounts_with_default_source(
    args: &AgentStartArgs,
    default_source: &Path,
) -> Vec<AgentMount> {
    let mut mounts = Vec::new();
    if args.default_workspace {
        mounts.push(AgentMount {
            source: default_source.display().to_string(),
            target: "/workspace".to_owned(),
            mode: "rw".to_owned(),
        });
        let git_dir = default_source.join(".git");
        if let Ok(metadata) = fs::symlink_metadata(&git_dir) {
            let file_type = metadata.file_type();
            if file_type.is_dir() || metadata.is_file() {
                mounts.push(AgentMount {
                    source: git_dir.display().to_string(),
                    target: "/workspace/.git".to_owned(),
                    mode: "ro".to_owned(),
                });
            }
        }
    }
    mounts.extend(args.mounts.iter().cloned());
    mounts
}

fn require_agent_mount(mount: &AgentMount) -> Result<(), CliError> {
    if !Path::new(&mount.source).is_absolute() {
        return Err(CliError::usage("agent mount source must be absolute"));
    }
    let Some(target) = normalized_absolute_mount_target(&mount.target) else {
        return Err(CliError::usage("agent mount target must be absolute"));
    };
    if !matches!(mount.mode.as_str(), "ro" | "rw") {
        return Err(CliError::usage("agent mount mode must be ro or rw"));
    }
    if is_protected_agent_mount_target(&target) {
        return Err(CliError::usage(
            "agent mount target cannot replace sandbox system paths",
        ));
    }
    Ok(())
}

fn normalized_absolute_mount_target(target: &str) -> Option<String> {
    let path = Path::new(target);
    if !path.is_absolute() {
        return None;
    }
    let mut normalized = PathBuf::from("/");
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::Normal(part) => normalized.push(part),
            std::path::Component::Prefix(_) => return None,
        }
    }
    Some(normalized.display().to_string())
}

fn is_protected_agent_mount_target(target: &str) -> bool {
    const PROTECTED_TARGETS: &[&str] = &[
        "/", "/bin", "/ctx", "/dev", "/etc", "/home", "/lib", "/lib64", "/proc", "/run",
        "/usr",
    ];

    PROTECTED_TARGETS
        .iter()
        .any(|protected| target == *protected || target.starts_with(&format!("{protected}/")))
}

fn require_sandbox_cwd(cwd: &str) -> Result<(), CliError> {
    if Path::new(cwd).is_absolute() {
        Ok(())
    } else {
        Err(CliError::usage("agent cwd must be absolute inside the sandbox"))
    }
}

fn ensure_agent_terminal_socket(
    visible_socket: &Path,
    runtime_socket: &Path,
) -> Result<(), CliError> {
    if let Some(parent) = runtime_socket.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            CliError::unavailable(format!("cannot create {}: {error}", parent.display()))
        })?;
        fs::write(parent.join(".empty-shell-startup"), "").map_err(|error| {
            CliError::unavailable(format!(
                "cannot create {}: {error}",
                parent.join(".empty-shell-startup").display()
            ))
        })?;
    }
    ensure_best_effort_visible_terminal_socket(visible_socket, runtime_socket)?;
    match fs::remove_file(runtime_socket) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(CliError::unavailable(format!(
                "cannot remove stale {}: {error}",
                runtime_socket.display()
            )));
        }
    }
    Ok(())
}

fn wait_for_agent_terminal_socket(socket: &Path) -> Result<(), CliError> {
    for _ in 0..50 {
        if socket.exists() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(CliError::unavailable(format!(
        "agent terminal service started, but socket did not appear: {}",
        socket.display()
    )))
}

fn agent_runtime_socket(root: &Path, name: &str, session: &str) -> Result<PathBuf, CliError> {
    let runtime_root = match env::var_os("XDG_RUNTIME_DIR") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from("/run")
            .join("user")
            .join(current_uid_for_ctx(root)?),
    };
    Ok(runtime_root
        .join("cortexfs")
        .join("terminal")
        .join(name)
        .join(session)
        .join("main.sock"))
}

fn agent_legacy_runtime_socket(root: &Path, name: &str, session: &str) -> Result<PathBuf, CliError> {
    Ok(PathBuf::from("/run")
        .join("cortexfs")
        .join("terminal")
        .join(current_uid_for_ctx(root)?)
        .join(name)
        .join(session)
        .join("main.sock"))
}

fn ensure_best_effort_visible_terminal_socket(
    visible_socket: &Path,
    runtime_socket: &Path,
) -> Result<(), CliError> {
    if let Some(parent) = visible_socket.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        if error.kind() == io::ErrorKind::PermissionDenied {
            return Ok(());
        }
        return Err(CliError::unavailable(format!(
            "cannot create {}: {error}",
            parent.display()
        )));
    }
    match fs::read_link(visible_socket) {
        Ok(target) if target == runtime_socket => Ok(()),
        Ok(_target) => Err(CliError::unavailable(format!(
            "{} already points at another socket",
            visible_socket.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match std::os::unix::fs::symlink(runtime_socket, visible_socket) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => Ok(()),
                Err(error) => Err(CliError::unavailable(format!(
                    "cannot create terminal socket link {} -> {}: {error}",
                    visible_socket.display(),
                    runtime_socket.display()
                ))),
            }
        }
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => Ok(()),
        Err(error) => Err(CliError::unavailable(format!(
            "cannot inspect {}: {error}",
            visible_socket.display()
        ))),
    }
}

fn current_uid_for_ctx(root: &Path) -> Result<String, CliError> {
    let home = ctx_home(root)?;
    home.file_name()
        .and_then(|uid| uid.to_str())
        .filter(|uid| uid.bytes().all(|byte| byte.is_ascii_digit()))
        .map(str::to_owned)
        .ok_or_else(|| CliError::unavailable("cannot derive uid from CTX_HOME"))
}

fn socket_runtime_dir(socket: &Path) -> Option<PathBuf> {
    socket_bind_path(socket).parent().map(Path::to_path_buf)
}

fn socket_bind_path(socket: &Path) -> PathBuf {
    match fs::read_link(socket) {
        Ok(target) if target.is_absolute() => target,
        Ok(target) => socket
            .parent()
            .map_or_else(|| target.clone(), |parent| parent.join(&target)),
        Err(_error) => socket.to_path_buf(),
    }
}

fn shell_startup_stub_path(socket: &Path) -> Option<PathBuf> {
    socket_runtime_dir(socket).map(|directory| directory.join(".empty-shell-startup"))
}

fn agent_terminal_unit(name: &str, session: &str) -> String {
    let session = session
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    format!("cortexfs-agent-{name}-{session}-terminal")
}

fn agent_terminal(
    root: &Path,
    name: &str,
    session: Option<&str>,
    write: bool,
) -> Result<ExitCode, CliError> {
    require_cli_name("agent name", name)?;
    let session = agent_session_name(root, name, session)?;
    require_session_name(&session)?;
    let socket = agent_terminal_connect_socket(root, name, &session)?;
    stream_terminal_socket(&socket, write, name, &session)
}

fn agent_terminal_socket(root: &Path, name: &str, session: &str) -> Result<PathBuf, CliError> {
    Ok(ctx_home(root)?
        .join("agent")
        .join(name)
        .join("session")
        .join(session)
        .join("terminal")
        .join("main.sock"))
}

fn agent_terminal_connect_socket(
    root: &Path,
    name: &str,
    session: &str,
) -> Result<PathBuf, CliError> {
    for socket in [
        agent_terminal_socket(root, name, session)?,
        agent_runtime_socket(root, name, session)?,
        agent_legacy_runtime_socket(root, name, session)?,
    ] {
        if socket.exists() {
            return Ok(socket);
        }
    }
    agent_terminal_socket(root, name, session)
}

fn require_session_name(session: &str) -> Result<(), CliError> {
    if !session.is_empty()
        && !matches!(session, "." | "..")
        && !session.contains('/')
        && !session.contains('\n')
        && !session.contains('\t')
    {
        Ok(())
    } else {
        Err(CliError::usage("invalid session name"))
    }
}

fn shell_quote_arg(value: &str) -> String {
    if value
        .bytes()
        .all(|byte| matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'/' | b':' | b'@' | b'%' | b'+' | b'=' | b','))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn stream_terminal_socket(
    socket: &Path,
    write: bool,
    name: &str,
    session: &str,
) -> Result<ExitCode, CliError> {
    let stream = open_terminal_socket(socket)
        .map_err(|error| terminal_connect_cli_error(socket, name, session, &error))?;
    stream_terminal_stream(stream, write)
}

fn open_terminal_socket(socket: &Path) -> Result<UnixStream, io::Error> {
    UnixStream::connect(socket)
}

fn terminal_connect_cli_error(
    socket: &Path,
    name: &str,
    session: &str,
    error: &io::Error,
) -> CliError {
    let hint = format!(
        "run: ctx agent start {} --session {}",
        shell_quote_arg(name),
        shell_quote_arg(session)
    );
    let reason = match error.kind() {
        io::ErrorKind::NotFound => "terminal is not running",
        io::ErrorKind::ConnectionRefused => "terminal socket exists but has no listener",
        _ => "cannot connect terminal socket",
    };
    CliError::unavailable(format!("{reason} {}: {error}\n{hint}", socket.display()))
}

fn stream_terminal_stream(mut stream: UnixStream, write: bool) -> Result<ExitCode, CliError> {
    if write {
        stream.write_all(b"attach\n")
    } else {
        stream.write_all(b"watch\n")
    }
    .map_err(|error| CliError::unavailable(format!("cannot write terminal mode: {error}")))?;

    let mut reader = stream
        .try_clone()
        .map_err(|error| CliError::unavailable(format!("cannot clone terminal socket: {error}")))?;
    let output = std::thread::spawn(move || copy_reader_to_stdout(&mut reader));
    if write {
        let _raw_mode = RawTerminalMode::maybe_new().map_err(|error| {
            CliError::unavailable(format!("cannot enter raw terminal mode: {error}"))
        })?;
        let input = std::thread::spawn(move || {
            let mut stdin = io::stdin().lock();
            let result = io::copy(&mut stdin, &mut stream);
            drop(stdin);
            let _ignored = stream.shutdown(Shutdown::Write);
            result
        });
        match input.join() {
            Ok(Ok(_bytes)) => {}
            Ok(Err(error)) if is_terminal_disconnect(&error) => {}
            Ok(Err(error)) => {
                return Err(CliError::unavailable(format!("terminal input failed: {error}")));
            }
            Err(_error) => return Err(CliError::unavailable("terminal input thread failed")),
        }
    }
    match output.join() {
        Ok(Ok(())) => {}
        Ok(Err(error)) if is_terminal_disconnect(&error) => {}
        Ok(Err(error)) => {
            return Err(CliError::unavailable(format!("terminal output failed: {error}")));
        }
        Err(_error) => return Err(CliError::unavailable("terminal output thread failed")),
    }
    Ok(ExitCode::SUCCESS)
}

fn copy_reader_to_stdout(mut reader: impl Read) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    let mut buffer = [0; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let Some(chunk) = buffer.get(..read) else {
            return Err(io::Error::other("terminal output read exceeded buffer"));
        };
        stdout.write_all(chunk)?;
        stdout.flush()?;
    }
    Ok(())
}

fn is_terminal_disconnect(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
    )
}

#[derive(Debug)]
struct RawTerminalMode {
    original: Termios,
}

impl RawTerminalMode {
    fn maybe_new() -> io::Result<Option<Self>> {
        let stdin = io::stdin();
        if !stdin.is_terminal() {
            return Ok(None);
        }
        let original = tcgetattr(stdin.as_fd()).map_err(nix_error_to_io)?;
        let mut raw = original.clone();
        cfmakeraw(&mut raw);
        tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &raw).map_err(nix_error_to_io)?;
        Ok(Some(Self { original }))
    }
}

impl Drop for RawTerminalMode {
    fn drop(&mut self) {
        let stdin = io::stdin();
        let _ignored = tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &self.original);
    }
}

fn nix_error_to_io(error: nix::errno::Errno) -> io::Error {
    io::Error::from(error)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AgentProcess {
    name: String,
    parent: Option<String>,
    status: String,
    pid: Option<String>,
}

fn agent_ps(root: &Path) -> Result<(), CliError> {
    let mut processes = read_agent_processes(root)?;
    processes.sort_by(|left, right| left.name.cmp(&right.name));
    let names = processes
        .iter()
        .map(|process| process.name.clone())
        .collect::<Vec<_>>();
    let mut rendered = Vec::new();
    for process in &processes {
        if process
            .parent
            .as_ref()
            .is_none_or(|parent| !names.contains(parent))
        {
            render_agent_process_tree(process, &processes, "", true, true, &mut rendered);
        }
    }
    for line in rendered {
        print_line(&line)?;
    }
    Ok(())
}

fn render_agent_process_tree(
    process: &AgentProcess,
    processes: &[AgentProcess],
    prefix: &str,
    last: bool,
    root: bool,
    rendered: &mut Vec<String>,
) {
    let branch = if root {
        ""
    } else if last {
        "`- "
    } else {
        "+- "
    };
    rendered.push(format!(
        "{prefix}{branch}{} [{}]{}",
        process.name,
        process.status,
        process
            .pid
            .as_ref()
            .map_or_else(String::new, |pid| format!(" pid={pid}"))
    ));

    let next_prefix = if root {
        String::new()
    } else if last {
        format!("{prefix}   ")
    } else {
        format!("{prefix}|  ")
    };
    let children = processes
        .iter()
        .filter(|candidate| candidate.parent.as_deref() == Some(process.name.as_str()))
        .collect::<Vec<_>>();
    let child_count = children.len();
    for (index, child) in children.into_iter().enumerate() {
        render_agent_process_tree(
            child,
            processes,
            &next_prefix,
            index + 1 == child_count,
            false,
            rendered,
        );
    }
}

fn read_agent_processes(root: &Path) -> Result<Vec<AgentProcess>, CliError> {
    let names = list_kind_names(root, ObjectClass::Agent)?;
    let mut processes = Vec::new();
    for name in names {
        let control = root.join("agent").join(format!("{name}.d"));
        processes.push(AgentProcess {
            name,
            parent: read_agent_parent(&control)?,
            status: read_agent_control_trimmed(&control, "status")?.unwrap_or_else(|| "unknown".to_owned()),
            pid: read_agent_control_trimmed(&control, "pid")?,
        });
    }
    Ok(processes)
}

fn read_agent_parent(control: &Path) -> Result<Option<String>, CliError> {
    let Some(parent) = read_agent_control_trimmed(control, "parent")? else {
        return Ok(None);
    };
    let Some(rest) = parent.strip_prefix("agent:") else {
        return Ok(None);
    };
    let name = rest.split_whitespace().next().unwrap_or_default();
    if is_object_name(name) {
        Ok(Some(name.to_owned()))
    } else {
        Ok(None)
    }
}

fn read_agent_control_trimmed(control: &Path, file: &str) -> Result<Option<String>, CliError> {
    let path = control.join(file);
    match fs::read_to_string(&path) {
        Ok(content) => {
            let value = content.trim().to_owned();
            Ok((!value.is_empty()).then_some(value))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(CliError::unavailable(format!(
            "cannot read {}: {error}",
            path.display()
        ))),
    }
}

fn agent_lifecycle_tool(root: &Path, name: &str, request: &str) -> Result<ExitCode, CliError> {
    let Some(hit) = ctx_tool_path(root)?.find(name).map_err(tool_path_error)? else {
        return Err(CliError::unavailable(format!(
            "agent lifecycle tool is not available: tool/{name}"
        )));
    };
    let status = ProcessCommand::new(hit.path())
        .arg(request)
        .status()
        .map_err(|error| {
            CliError::unavailable(format!("cannot exec {}: {error}", hit.path().display()))
        })?;
    Ok(status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or_else(|| ExitCode::from(70), ExitCode::from))
}

fn agent_new_request_json(args: &AgentNewArgs) -> Result<String, CliError> {
    require_cli_name("agent name", &args.name)?;
    if let Some(label) = args.label.as_deref()
        && label.trim().is_empty()
    {
        return Err(CliError::usage("agent label must not be empty"));
    }
    for model in &args.models {
        if !is_model_name(model) {
            return Err(CliError::usage(format!("invalid model name: {model}")));
        }
    }
    for tool in &args.tools {
        require_cli_name("tool name", tool)?;
    }
    for shared in &args.shared {
        require_cli_name("shared name", &shared.name)?;
        if !matches!(shared.access.as_str(), "read" | "write") {
            return Err(CliError::usage(format!(
                "invalid shared access for {}: {}",
                shared.name, shared.access
            )));
        }
    }
    for mount in &args.mounts {
        if !is_absolute_small_path(&mount.source) || !is_absolute_small_path(&mount.target) {
            return Err(CliError::usage("agent mount paths must be absolute"));
        }
        if !matches!(mount.mode.as_str(), "ro" | "rw") {
            return Err(CliError::usage(format!(
                "invalid mount mode for {}: {}",
                mount.target, mount.mode
            )));
        }
    }

    let mut fields = vec![format!("\"name\":{}", json_string(&args.name))];
    if args.temporary {
        fields.push("\"life\":\"temp\"".to_owned());
    }
    if let Some(label) = args.label.as_deref() {
        fields.push(format!("\"label\":{}", json_string(label)));
    }
    if !args.models.is_empty() {
        fields.push(format!("\"model\":[{}]", json_string_list(&args.models)));
    }
    if !args.tools.is_empty() {
        fields.push(format!("\"tools\":[{}]", json_string_list(&args.tools)));
    }
    if !args.shared.is_empty() {
        fields.push(format!("\"shared\":{}", agent_shared_json(&args.shared)));
    }
    if !args.mounts.is_empty() {
        fields.push(format!("\"mount\":{}", agent_mounts_json(&args.mounts)));
    }
    Ok(format!("{{{}}}", fields.join(",")))
}

fn agent_name_request_json(name: &str) -> String {
    format!("{{\"name\":{}}}", json_string(name))
}

fn agent_shared_json(shared: &[AgentShared]) -> String {
    let fields = shared
        .iter()
        .map(|entry| {
            format!(
                "{}:[{}]",
                json_string(&entry.name),
                json_string(&entry.access)
            )
        })
        .collect::<Vec<_>>();
    format!("{{{}}}", fields.join(","))
}

fn agent_mounts_json(mounts: &[AgentMount]) -> String {
    let items = mounts
        .iter()
        .map(|mount| {
            format!(
                "[{},{},{}]",
                json_string(&mount.source),
                json_string(&mount.target),
                json_string(&mount.mode)
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", items.join(","))
}

fn json_string_list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| json_string(value))
        .collect::<Vec<_>>()
        .join(",")
}

fn is_absolute_small_path(value: &str) -> bool {
    value.starts_with('/')
        && !value.contains('\0')
        && !value.contains('\n')
        && !value.contains('\t')
}
