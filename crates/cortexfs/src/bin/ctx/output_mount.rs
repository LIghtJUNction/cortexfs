fn print_help() -> Result<(), CliError> {
    print_lines(&[
        "ctx - CortexFS filesystem management CLI",
        "",
        "usage:",
        "  ctx [--root PATH] status",
        "  ctx [--root PATH] abi",
        "  ctx [--root PATH] env",
        "  ctx [--root PATH] root",
        "  ctx bootstrap [SOURCE]",
        "  ctx [--root PATH] mount [--source SOURCE] [MOUNTPOINT]",
        "  ctx [--root PATH] ls [PATH|model|agent|tool]",
        "  ctx [--root PATH] which model|agent|tool NAME",
        "  ctx [--root PATH] path shared NAME",
        "  ctx [--root PATH] history AGENT [SESSION]",
        "  ctx [--root PATH] resume AGENT [SESSION]",
        "  ctx [--root PATH] send AGENT SESSION INPUT",
        "  ctx [--root PATH] agent new NAME [--temp] [--label LABEL] [--model MODEL] [--tool TOOL] [--shared NAME:read|write] [--mount SOURCE TARGET ro|rw]",
        "  ctx [--root PATH] agent start NAME [--session SESSION] [--cwd PATH] [--mount SOURCE TARGET ro|rw] [--no-default-workspace]",
        "  ctx [--root PATH] agent stop NAME",
        "  ctx [--root PATH] agent status NAME",
        "  ctx [--root PATH] agent ps",
        "  ctx [--root PATH] agent send NAME [--session SESSION] [--raw] INPUT",
        "  ctx [--root PATH] agent repl NAME [--session SESSION] [--raw]",
        "  ctx [--root PATH] agent resume NAME [--session SESSION] [--raw]",
        "  ctx [--root PATH] agent history NAME [--session SESSION]",
        "  ctx [--root PATH] agent output NAME [--session SESSION]",
        "  ctx [--root PATH] agent pack NAME [--session SESSION]",
        "  ctx [--root PATH] agent prompt NAME",
        "  ctx [--root PATH] agent tools NAME",
        "  ctx [--root PATH] agent children NAME [--session SESSION]",
        "  ctx [--root PATH] agent cancel NAME [--session SESSION] [--raw] [RUN]",
        "  ctx [--root PATH] agent watch NAME [--session SESSION]",
        "  ctx [--root PATH] agent attach NAME [--session SESSION]",
        "  ctx [--root PATH] ping model/NAME|agent/NAME",
        "  ctx [--root PATH] cancel model/NAME|agent/NAME RUN",
        "  ctx [--root PATH] exec model/NAME|agent/NAME|tool/NAME [ARG...]",
        "  ctx [--root PATH] tool NAME [ARG...]",
        "  ctx [--root PATH] cat PATH",
        "  ctx [--root PATH] set PATH VALUE",
        "  ctx [--root PATH] append PATH VALUE",
        "  ctx [--root PATH] file PATH",
        "  ctx [--root PATH] file info PATH",
        "  ctx [--root PATH] file type PATH",
        "  ctx [--root PATH] file check PATH",
        "  ctx [--root PATH] doctor",
        "  ctx validate-name NAME",
        "",
        "principles:",
        "  ctx is a thin Unix client over /ctx",
        "  ctx does not manage providers, API formats, or private sessions",
    ])
}

#[expect(clippy::too_many_lines, reason = "CLI help topic table is intentionally flat")]
fn print_help_topic(topic: &str) -> Result<(), CliError> {
    match topic {
        "status" => print_lines(&["usage:", "  ctx [--root PATH] status"]),
        "abi" => print_lines(&["usage:", "  ctx [--root PATH] abi"]),
        "env" => print_lines(&["usage:", "  ctx [--root PATH] env"]),
        "root" => print_lines(&["usage:", "  ctx [--root PATH] root"]),
        "bootstrap" => print_lines(&["usage:", "  ctx bootstrap [SOURCE]"]),
        "mount" => print_lines(&[
            "usage:",
            "  ctx [--root PATH] mount [--source SOURCE] [MOUNTPOINT]",
        ]),
        "ls" => print_lines(&["usage:", "  ctx [--root PATH] ls [PATH|model|agent|tool]"]),
        "which" => print_lines(&["usage:", "  ctx [--root PATH] which model|agent|tool NAME"]),
        "which-tool" => print_lines(&["usage:", "  ctx [--root PATH] which-tool NAME"]),
        "path" => print_lines(&["usage:", "  ctx [--root PATH] path shared NAME"]),
        "history" => print_lines(&["usage:", "  ctx [--root PATH] history AGENT [SESSION]"]),
        "resume" => print_lines(&["usage:", "  ctx [--root PATH] resume AGENT [SESSION]"]),
        "send" => print_lines(&["usage:", "  ctx [--root PATH] send AGENT SESSION INPUT"]),
        "agent" => print_lines(&[
            "usage:",
            "  ctx [--root PATH] agent new NAME [--temp] [--label LABEL] [--model MODEL] [--tool TOOL] [--shared NAME:read|write] [--mount SOURCE TARGET ro|rw]",
            "  ctx [--root PATH] agent start NAME [--session SESSION] [--cwd PATH] [--mount SOURCE TARGET ro|rw] [--no-default-workspace]",
            "  ctx [--root PATH] agent stop NAME",
            "  ctx [--root PATH] agent status NAME",
            "  ctx [--root PATH] agent ps",
            "  ctx [--root PATH] agent send NAME [--session SESSION] [--raw] INPUT",
            "  ctx [--root PATH] agent repl NAME [--session SESSION] [--raw]",
            "  ctx [--root PATH] agent resume NAME [--session SESSION] [--raw]",
            "  ctx [--root PATH] agent history NAME [--session SESSION]",
            "  ctx [--root PATH] agent output NAME [--session SESSION]",
            "  ctx [--root PATH] agent pack NAME [--session SESSION]",
            "  ctx [--root PATH] agent prompt NAME",
            "  ctx [--root PATH] agent tools NAME",
            "  ctx [--root PATH] agent children NAME [--session SESSION]",
            "  ctx [--root PATH] agent cancel NAME [--session SESSION] [--raw] [RUN]",
            "  ctx [--root PATH] agent watch NAME [--session SESSION]",
            "  ctx [--root PATH] agent attach NAME [--session SESSION]",
        ]),
        "agent new" => print_lines(&[
            "usage:",
            "  ctx [--root PATH] agent new NAME [--temp] [--label LABEL] [--model MODEL] [--tool TOOL] [--shared NAME:read|write] [--mount SOURCE TARGET ro|rw]",
        ]),
        "agent start" => print_lines(&[
            "usage:",
            "  ctx [--root PATH] agent start NAME [--session SESSION] [--cwd PATH] [--mount SOURCE TARGET ro|rw] [--no-default-workspace]",
            "",
            "default:",
            "  binds the caller current directory to /workspace rw",
            "  starts ctxterm -> tsh inside a bwrap sandbox at /workspace",
        ]),
        "agent stop" => print_lines(&["usage:", "  ctx [--root PATH] agent stop NAME"]),
        "agent status" => print_lines(&["usage:", "  ctx [--root PATH] agent status NAME"]),
        "agent ps" => print_lines(&["usage:", "  ctx [--root PATH] agent ps"]),
        "agent send" => print_lines(&[
            "usage:",
            "  ctx [--root PATH] agent send NAME [--session SESSION] [--raw] INPUT",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
        ]),
        "agent repl" => print_lines(&[
            "usage:",
            "  ctx [--root PATH] agent repl NAME [--session SESSION] [--raw]",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
        ]),
        "agent resume" => print_lines(&[
            "usage:",
            "  ctx [--root PATH] agent resume NAME [--session SESSION] [--raw]",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
        ]),
        "agent history" => print_lines(&[
            "usage:",
            "  ctx [--root PATH] agent history NAME [--session SESSION]",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
        ]),
        "agent output" => print_lines(&[
            "usage:",
            "  ctx [--root PATH] agent output NAME [--session SESSION]",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
        ]),
        "agent pack" => print_lines(&[
            "usage:",
            "  ctx [--root PATH] agent pack NAME [--session SESSION]",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
        ]),
        "agent prompt" => print_lines(&[
            "usage:",
            "  ctx [--root PATH] agent prompt NAME",
            "",
            "prints the rendered runtime system prompt for the agent",
        ]),
        "agent tools" => print_lines(&["usage:", "  ctx [--root PATH] agent tools NAME"]),
        "agent children" => print_lines(&[
            "usage:",
            "  ctx [--root PATH] agent children NAME [--session SESSION]",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
        ]),
        "agent cancel" => print_lines(&[
            "usage:",
            "  ctx [--root PATH] agent cancel NAME [--session SESSION] [--raw] [RUN]",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
        ]),
        "agent watch" => print_lines(&[
            "usage:",
            "  ctx [--root PATH] agent watch NAME [--session SESSION]",
        ]),
        "agent attach" => print_lines(&[
            "usage:",
            "  ctx [--root PATH] agent attach NAME [--session SESSION]",
        ]),
        "ping" => print_lines(&["usage:", "  ctx [--root PATH] ping model/NAME|agent/NAME"]),
        "cancel" => print_lines(&["usage:", "  ctx [--root PATH] cancel model/NAME|agent/NAME RUN"]),
        "exec" => print_lines(&[
            "usage:",
            "  ctx [--root PATH] exec model/NAME|agent/NAME|tool/NAME [ARG...]",
        ]),
        "tool" => print_lines(&[
            "usage:",
            "  ctx [--root PATH] tool NAME [ARG...]",
            "",
            "lookup:",
            "  validates NAME through CTX_PATH but refuses direct execution without CortexFS authorization",
        ]),
        "cat" => print_lines(&["usage:", "  ctx [--root PATH] cat PATH"]),
        "set" => print_lines(&["usage:", "  ctx [--root PATH] set PATH VALUE"]),
        "append" => print_lines(&["usage:", "  ctx [--root PATH] append PATH VALUE"]),
        "file" => print_lines(&[
            "usage:",
            "  ctx [--root PATH] file PATH",
            "  ctx [--root PATH] file info PATH",
            "  ctx [--root PATH] file type PATH",
            "  ctx [--root PATH] file check PATH",
            "",
            "output:",
            "  file PATH prints CortexFS type, stat, token estimate, and user.cortexfs.* xattrs",
        ]),
        "doctor" => print_lines(&["usage:", "  ctx [--root PATH] doctor"]),
        "validate-name" => print_lines(&["usage:", "  ctx validate-name NAME"]),
        _ => Err(CliError::usage(format!("unknown help topic: {topic}"))),
    }
}

fn print_abi() -> Result<(), CliError> {
    print_line("root=/ctx")?;
    print_line("entries=status bin model agent tool home shared")?;
    print_line("exec=model agent tool")?;
    print_line("socket=name.sock")?;
    print_line("control=name.d")?;
    print_line("policy=allow <subject_type> <object_class>:<object_name> <permission>")
}

fn print_env(root: &Path) -> Result<(), CliError> {
    for line in env_exports(
        root,
        env::var("CTX_HOME").ok().as_deref(),
        env::var("CTX_PATH").ok().as_deref(),
    ) {
        print_line(&line)?;
    }
    Ok(())
}

fn env_exports(root: &Path, home_env: Option<&str>, path_env: Option<&str>) -> [String; 4] {
    let root = root.display().to_string();
    let home = home_env.map_or_else(|| format!("{root}/home/$(id -u)"), str::to_owned);
    let path = path_env.map_or_else(|| format!("{root}/tool:{home}/tool"), str::to_owned);
    let root_bin = format!("{root}/bin");
    [
        format!("export CTX_ROOT={}", shell_quote(&root)),
        format!("export CTX_HOME={}", shell_quote(&home)),
        format!("export CTX_PATH={}", shell_quote(&path)),
        format!("export PATH={}:$PATH", shell_quote(&root_bin)),
    ]
}

fn print_status(root: &Path) -> Result<(), CliError> {
    let exists = root.exists();
    let is_dir = root.is_dir();
    let mounted = is_mount_point(root).unwrap_or(false);
    let status = read_ctx_status(root);
    let present_entries = ROOT_ENTRIES
        .iter()
        .filter(|entry| root.join(entry).exists())
        .count();
    let missing_entries = ROOT_ENTRIES.len().saturating_sub(present_entries);
    let agents = read_status_agent_processes(root)?;

    print_line("ctx")?;
    print_line(&format!("    State: {}", ctx_state(exists, is_dir, mounted)))?;
    print_line(&format!("     Root: {}", root.display()))?;
    print_line(&format!("   Status: {status}"))?;
    print_line(&format!(
        "  Mounted: {}",
        if mounted { "yes" } else { "no" }
    ))?;
    print_line(&format!(
        "  Entries: {present_entries}/{} loaded",
        ROOT_ENTRIES.len()
    ))?;
    print_line(&format!("   Failed: {missing_entries} entries"))?;
    print_line(&format!("   Agents: {} loaded", agents.len()))?;

    if !agents.is_empty() {
        print_line("    Tree:")?;
        for line in render_agent_status_lines(&agents) {
            print_line(&format!("          {line}"))?;
        }
    }

    Ok(())
}

fn ctx_state(exists: bool, is_dir: bool, mounted: bool) -> &'static str {
    if mounted {
        "running"
    } else if exists && is_dir {
        "available"
    } else if exists {
        "invalid"
    } else {
        "missing"
    }
}

fn read_ctx_status(root: &Path) -> String {
    fs::read_to_string(root.join("status"))
        .ok()
        .map(|content| content.trim().to_owned())
        .filter(|content| !content.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn read_status_agent_processes(root: &Path) -> Result<Vec<AgentProcess>, CliError> {
    match read_agent_processes(root) {
        Ok(processes) => Ok(processes),
        Err(error)
            if error.message.starts_with(&format!(
                "cannot read {}",
                root.join("agent").display()
            )) =>
        {
            Ok(Vec::new())
        }
        Err(error) => Err(error),
    }
}

fn render_agent_status_lines(processes: &[AgentProcess]) -> Vec<String> {
    let mut processes = processes.to_vec();
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
    rendered
}

fn bootstrap_reference_tree(source: Option<&Path>) -> Result<(), CliError> {
    let source = match source {
        Some(path) => path.to_path_buf(),
        None => default_source_root()?,
    };
    ensure_v1_reference_tree(&source).map_err(|error| {
        CliError::unavailable(format!(
            "cannot bootstrap {}: {}",
            source.display(),
            error.errno()
        ))
    })?;
    print_line(&format!("source={}", source.display()))
}

fn mount_reference_tree(
    root: &Path,
    source: Option<&Path>,
    mountpoint: Option<&Path>,
) -> Result<(), CliError> {
    let source = match source {
        Some(path) => path.to_path_buf(),
        None => default_source_root()?,
    };
    let mountpoint = mountpoint.unwrap_or(root);

    ensure_v1_reference_tree(&source).map_err(|error| {
        CliError::unavailable(format!(
            "cannot bootstrap {}: {}",
            source.display(),
            error.errno()
        ))
    })?;
    fs::create_dir_all(mountpoint).map_err(|error| {
        CliError::unavailable(format!(
            "cannot create mountpoint {}: {error}",
            mountpoint.display()
        ))
    })?;
    let mountpoint = absolute_existing_path(mountpoint).map_err(|error| {
        CliError::unavailable(format!(
            "cannot resolve mountpoint {}: {error}",
            mountpoint.display()
        ))
    })?;
    if is_mount_point(&mountpoint).unwrap_or(false) {
        return Err(CliError::unavailable(format!(
            "already mounted: {}",
            mountpoint.display()
        )));
    }

    let mount_bin = cortexfs_mount_bin();
    spawn_mount_process(&mount_bin, &source, &mountpoint)?;

    for _attempt in 0..20 {
        if is_mount_point(&mountpoint).unwrap_or(false) {
            print_line(&format!("mounted={}", mountpoint.display()))?;
            print_line(&format!("source={}", source.display()))?;
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    Err(CliError::unavailable(format!(
        "mount did not become ready: {}",
        mountpoint.display()
    )))
}

fn default_source_root() -> Result<PathBuf, CliError> {
    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(data_home).join("cortexfs").join("v1-root"));
    }
    if let Some(home) = env::var_os("HOME") {
        return Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("cortexfs")
            .join("v1-root"));
    }
    Err(CliError::unavailable(
        "cannot choose default source root without HOME or XDG_DATA_HOME",
    ))
}

fn cortexfs_mount_bin() -> PathBuf {
    if let Ok(current) = env::current_exe()
        && let Some(dir) = current.parent()
    {
        let sibling = dir.join("cortexfs-mount");
        if sibling.is_file() {
            return sibling;
        }
    }
    PathBuf::from("cortexfs-mount")
}

const TRUSTED_SETSID_BIN: &str = "/usr/bin/setsid";

fn spawn_mount_process(mount_bin: &Path, source: &Path, mountpoint: &Path) -> Result<(), CliError> {
    let mut detached = ProcessCommand::new(TRUSTED_SETSID_BIN);
    detached
        .arg("-f")
        .arg(mount_bin)
        .arg("--source")
        .arg(source)
        .arg(mountpoint);
    match spawn_null(detached) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut direct = ProcessCommand::new(mount_bin);
            direct.arg("--source").arg(source).arg(mountpoint);
            spawn_null(direct).map_err(|error| {
                CliError::unavailable(format!("cannot start {}: {error}", mount_bin.display()))
            })
        }
        Err(error) => Err(CliError::unavailable(format!(
            "cannot start {}: {error}",
            mount_bin.display()
        ))),
    }
}

fn spawn_null(mut command: ProcessCommand) -> io::Result<()> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_child| ())
}
