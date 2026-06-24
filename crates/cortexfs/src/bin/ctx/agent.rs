#[derive(Debug, Eq, PartialEq)]
enum AgentArgs {
    New(AgentNewArgs),
    Start(AgentStartArgs),
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

fn agent_start(root: &Path, args: &AgentStartArgs) -> Result<ExitCode, CliError> {
    require_cli_name("agent name", &args.name)?;
    require_session_name(&args.session)?;
    require_sandbox_cwd(&args.cwd)?;
    let mounts = agent_start_mounts(args)?;
    for mount in &mounts {
        require_agent_mount(mount)?;
    }
    let socket = agent_terminal_socket(root, &args.name, &args.session)?;
    ensure_agent_terminal_socket(root, &args.name, &args.session, &socket)?;
    let unit = agent_terminal_unit(&args.name, &args.session);
    let command = agent_start_systemd_command(root, args, &mounts, &socket, &unit)?;
    let status = ProcessCommand::new(&command.program)
        .args(&command.args)
        .status()
        .map_err(|error| CliError::unavailable(format!("cannot start systemd-run: {error}")))?;
    if !status.success() {
        return Err(CliError::unavailable(format!(
            "agent terminal service failed to start with {status}"
        )));
    }
    print_line(&format!("agent={}", args.name))?;
    print_line(&format!("session={}", args.session))?;
    print_line(&format!("unit={unit}.service"))?;
    print_line(&format!("cwd={}", args.cwd))?;
    print_line(&format!("socket={}", socket.display()))?;
    Ok(ExitCode::SUCCESS)
}

#[derive(Debug, Eq, PartialEq)]
struct AgentStartCommand {
    program: String,
    args: Vec<String>,
}

fn agent_start_systemd_command(
    root: &Path,
    args: &AgentStartArgs,
    mounts: &[AgentMount],
    socket: &Path,
    unit: &str,
) -> Result<AgentStartCommand, CliError> {
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
            format!("CTX_ROOT={}", root.display()),
            format!("CTX_HOME={}", ctx_home(root)?.display()),
            format!("HOME={}", args.cwd),
            format!("USER={}", args.name),
            format!("LOGNAME={}", args.name),
            "SHELL=/usr/bin/bash".to_owned(),
            "TERM=xterm-256color".to_owned(),
            "LANG=C.UTF-8".to_owned(),
            "/usr/bin/bwrap".to_owned(),
        ],
    };
    command.args.extend(agent_bwrap_args(root, args, mounts, socket));
    Ok(command)
}

fn agent_bwrap_args(
    root: &Path,
    args: &AgentStartArgs,
    mounts: &[AgentMount],
    socket: &Path,
) -> Vec<String> {
    let mut bwrap = vec![
        "--die-with-parent".to_owned(),
        "--unshare-pid".to_owned(),
        "--proc".to_owned(),
        "/proc".to_owned(),
        "--dev".to_owned(),
        "/dev".to_owned(),
        "--tmpfs".to_owned(),
        "/tmp".to_owned(),
        "--dir".to_owned(),
        "/run".to_owned(),
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
        "--bind".to_owned(),
        root.display().to_string(),
        root.display().to_string(),
    ];
    if let Some(runtime_dir) = socket_runtime_dir(socket) {
        bwrap.extend([
            "--bind".to_owned(),
            runtime_dir.display().to_string(),
            runtime_dir.display().to_string(),
        ]);
    }
    for mount in mounts {
        bwrap.push(if mount.mode == "ro" {
            "--ro-bind".to_owned()
        } else {
            "--bind".to_owned()
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
    let mut mounts = Vec::new();
    if args.default_workspace {
        mounts.push(AgentMount {
            source: env::current_dir()
                .map_err(|error| {
                    CliError::unavailable(format!("cannot read current directory: {error}"))
                })?
                .display()
                .to_string(),
            target: "/workspace".to_owned(),
            mode: "rw".to_owned(),
        });
    }
    mounts.extend(args.mounts.iter().cloned());
    Ok(mounts)
}

fn require_agent_mount(mount: &AgentMount) -> Result<(), CliError> {
    if !Path::new(&mount.source).is_absolute() {
        return Err(CliError::usage("agent mount source must be absolute"));
    }
    if !Path::new(&mount.target).is_absolute() {
        return Err(CliError::usage("agent mount target must be absolute"));
    }
    if !matches!(mount.mode.as_str(), "ro" | "rw") {
        return Err(CliError::usage("agent mount mode must be ro or rw"));
    }
    if mount.target == "/" || mount.target.starts_with("/ctx/") || mount.target == "/ctx" {
        return Err(CliError::usage("agent mount target cannot replace / or /ctx"));
    }
    Ok(())
}

fn require_sandbox_cwd(cwd: &str) -> Result<(), CliError> {
    if Path::new(cwd).is_absolute() {
        Ok(())
    } else {
        Err(CliError::usage("agent cwd must be absolute inside the sandbox"))
    }
}

fn ensure_agent_terminal_socket(
    root: &Path,
    name: &str,
    session: &str,
    socket: &Path,
) -> Result<(), CliError> {
    if let Some(parent) = socket.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            CliError::unavailable(format!("cannot create {}: {error}", parent.display()))
        })?;
    }
    let runtime_socket = agent_runtime_socket(root, name, session)?;
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
    match fs::read_link(socket) {
        Ok(target) if target == runtime_socket => {}
        Ok(_target) => {
            return Err(CliError::unavailable(format!(
                "{} already points at another socket",
                socket.display()
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            std::os::unix::fs::symlink(&runtime_socket, socket).map_err(|error| {
                CliError::unavailable(format!(
                    "cannot create terminal socket link {} -> {}: {error}",
                    socket.display(),
                    runtime_socket.display()
                ))
            })?;
        }
        Err(error) => {
            return Err(CliError::unavailable(format!(
                "cannot inspect {}: {error}",
                socket.display()
            )));
        }
    }
    match fs::remove_file(&runtime_socket) {
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

fn agent_runtime_socket(root: &Path, name: &str, session: &str) -> Result<PathBuf, CliError> {
    Ok(PathBuf::from("/run")
        .join("cortexfs")
        .join("terminal")
        .join(current_uid_for_ctx(root)?)
        .join(name)
        .join(session)
        .join("main.sock"))
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
    session: &str,
    write: bool,
) -> Result<ExitCode, CliError> {
    require_cli_name("agent name", name)?;
    require_session_name(session)?;
    let socket = agent_terminal_socket(root, name, session)?;
    stream_terminal_socket(&socket, write, name, session)
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
    if is_usage_placeholder(session) {
        return Err(CliError::usage(format!(
            "session name is a placeholder; replace {session} with a real value"
        )));
    }
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

fn stream_terminal_socket(
    socket: &Path,
    write: bool,
    name: &str,
    session: &str,
) -> Result<ExitCode, CliError> {
    let mut stream = UnixStream::connect(socket).map_err(|error| {
        let hint = format!("run: ctx agent start {name} --session {session}");
        let reason = match error.kind() {
            io::ErrorKind::NotFound => "terminal is not running",
            io::ErrorKind::ConnectionRefused => "terminal socket exists but has no listener",
            _ => "cannot connect terminal socket",
        };
        CliError::unavailable(format!(
            "{reason} {}: {error}\n{hint}",
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
