#[derive(Debug, Eq, PartialEq)]
enum AgentArgs {
    New(AgentNewArgs),
    Start { name: String },
    Stop { name: String },
    Status { name: String },
    Ps,
    Watch { name: String, session: String },
    Attach { name: String, session: String },
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
struct AgentShared {
    name: String,
    access: String,
}

#[derive(Debug, Eq, PartialEq)]
struct AgentMount {
    source: String,
    target: String,
    mode: String,
}

fn agent_command(root: &Path, args: &AgentArgs) -> Result<ExitCode, CliError> {
    match *args {
        AgentArgs::New(ref args) => agent_lifecycle_tool(
            root,
            "agent.create",
            &agent_new_request_json(args)?,
        ),
        AgentArgs::Start { ref name } => {
            require_cli_name("agent name", name)?;
            agent_lifecycle_tool(root, "agent.start", &agent_name_request_json(name))
        }
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
        AgentArgs::Watch {
            ref name,
            ref session,
        } => agent_terminal(root, name, session, false),
        AgentArgs::Attach {
            ref name,
            ref session,
        } => agent_terminal(root, name, session, true),
    }
}

fn agent_terminal(
    root: &Path,
    name: &str,
    session: &str,
    write: bool,
) -> Result<ExitCode, CliError> {
    require_cli_name("agent name", name)?;
    require_session_name(session)?;
    let socket = agent_terminal_socket(root, name, session)?;
    stream_terminal_socket(&socket, write)
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

fn stream_terminal_socket(socket: &Path, write: bool) -> Result<ExitCode, CliError> {
    let mut stream = UnixStream::connect(socket).map_err(|error| {
        CliError::unavailable(format!(
            "cannot connect terminal socket {}: {error}",
            socket.display()
        ))
    })?;
    if write {
        stream.write_all(b"attach\n")
    } else {
        stream.write_all(b"watch\n")
    }
    .map_err(|error| CliError::unavailable(format!("cannot write terminal mode: {error}")))?;

    let mut reader = stream
        .try_clone()
        .map_err(|error| CliError::unavailable(format!("cannot clone terminal socket: {error}")))?;
    let output = std::thread::spawn(move || {
        let mut stdout = io::stdout().lock();
        io::copy(&mut reader, &mut stdout).and_then(|_bytes| stdout.flush())
    });
    if write {
        let input = std::thread::spawn(move || {
            let mut stdin = io::stdin().lock();
            let result = io::copy(&mut stdin, &mut stream);
            drop(stdin);
            let _ignored = stream.shutdown(Shutdown::Write);
            result
        });
        match input.join() {
            Ok(Ok(_bytes)) => {}
            Ok(Err(error)) => {
                return Err(CliError::unavailable(format!("terminal input failed: {error}")));
            }
            Err(_error) => return Err(CliError::unavailable("terminal input thread failed")),
        }
    }
    match output.join() {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            return Err(CliError::unavailable(format!("terminal output failed: {error}")));
        }
        Err(_error) => return Err(CliError::unavailable("terminal output thread failed")),
    }
    Ok(ExitCode::SUCCESS)
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
