use crate::*;

pub(crate) fn agent_stop_host_fallback(root: &Path, name: &str) -> Result<ExitCode, CliError> {
    let plan = plan_agent_stop(root, name)?;
    execute_agent_stop(root, plan)?;
    print_line(&format!("agent {} stopped", terminal_safe_text(name)))?;
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn agent_terminal_units(root: &Path, name: &str) -> Result<Vec<String>, CliError> {
    let session_root = ctx_home(root)?.join("agent").join(name).join("session");
    if !session_root
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.is_dir())
    {
        return Ok(Vec::new());
    }
    let mut units = read_dir_names(&session_root)?
        .into_iter()
        .filter(|session| is_object_name(session))
        .map(|session| agent_terminal_unit(name, &session))
        .collect::<Vec<_>>();
    units.sort();
    Ok(units)
}

pub(crate) fn stop_agent_control(control: &Path, name: &str) -> Result<(), CliError> {
    let metadata = fs::symlink_metadata(control).map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", control.display()))
    })?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(CliError::unavailable(format!(
            "agent control is not a plain directory: {}",
            control.display()
        )));
    }
    write_agent_control_plain(&control.join("status"), "dead\n")?;
    write_agent_control_plain(&control.join("pid"), "\n")?;
    append_agent_log_event(&control.join("log"), &agent_stop_log_event(name))
}

struct StopCandidate {
    name: String,
    control: PathBuf,
    parent: Option<AgentParentRef>,
}

struct StopPlan {
    agents: Vec<PlannedStop>,
}

struct PlannedStop {
    name: String,
    terminal_units: Vec<String>,
    cancellations: Vec<PlannedCancellation>,
    cleanup: Option<TempCleanupPlan>,
}

struct PlannedCancellation {
    parent_session: PathBuf,
    child: String,
}

struct TempCleanupPlan {
    entries: Vec<TempCleanupEntry>,
}

struct TempCleanupEntry {
    path: PathBuf,
    directory: bool,
}

fn plan_agent_stop(root: &Path, root_agent: &str) -> Result<StopPlan, CliError> {
    let mut candidates = Vec::new();
    for (name, control) in agent_control_dirs(root)? {
        if RETIRED_REFERENCE_AGENTS.contains(&name.as_str()) && name != root_agent {
            continue;
        }
        candidates.push(StopCandidate {
            parent: read_agent_parent_ref(&control)?,
            name,
            control,
        });
    }
    candidates.sort_by(|left, right| left.name.cmp(&right.name));

    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut agents = Vec::new();
    plan_stop_subtree(
        root,
        root_agent,
        None,
        false,
        &candidates,
        &mut visiting,
        &mut visited,
        &mut agents,
    )?;
    Ok(StopPlan { agents })
}

#[expect(
    clippy::too_many_arguments,
    reason = "the stop DFS keeps its complete validation state explicit"
)]
fn plan_stop_subtree(
    root: &Path,
    name: &str,
    parent: Option<&AgentParentRef>,
    temporary: bool,
    candidates: &[StopCandidate],
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
    agents: &mut Vec<PlannedStop>,
) -> Result<(), CliError> {
    if !visiting.insert(name.to_owned()) {
        return Err(CliError::unavailable(format!(
            "agent stop ownership cycle at {name}"
        )));
    }
    if visited.contains(name) {
        return Err(CliError::unavailable(format!(
            "duplicate agent in stop plan: {name}"
        )));
    }

    let control = agent_control_dir(root, name);
    preflight_stop_control(&control)?;
    let terminal_units = agent_terminal_units(root, name)?;
    let cancellations = parent.map_or_else(
        || Ok(Vec::new()),
        |parent| plan_parent_child_cancellations(root, name, parent),
    )?;
    let cleanup = if temporary && is_dedicated_worker_agent_name(name) {
        Some(plan_temp_cleanup(root, name)?)
    } else {
        None
    };

    for child in candidates.iter().filter(|candidate| {
        candidate
            .parent
            .as_ref()
            .is_some_and(|parent| parent.agent == name)
    }) {
        if RETIRED_REFERENCE_AGENTS.contains(&child.name.as_str()) {
            continue;
        }
        let life = read_agent_control_trimmed(&child.control, "life")?;
        let temporary = match life.as_deref() {
            None | Some("owned") => false,
            Some("temp") => true,
            Some(life) => {
                return Err(CliError::usage(format!(
                    "invalid agent life for {}: {life}",
                    child.name
                )));
            }
        };
        plan_stop_subtree(
            root,
            &child.name,
            child.parent.as_ref(),
            temporary,
            candidates,
            visiting,
            visited,
            agents,
        )?;
    }

    visiting.remove(name);
    visited.insert(name.to_owned());
    agents.push(PlannedStop {
        name: name.to_owned(),
        terminal_units,
        cancellations,
        cleanup,
    });
    Ok(())
}

fn execute_agent_stop(root: &Path, plan: StopPlan) -> Result<(), CliError> {
    for agent in plan.agents {
        reset_agent_chat_unit(&agent_chat_unit(root, &agent.name));
        for unit in agent.terminal_units {
            reset_agent_terminal_unit(&unit);
        }
        stop_agent_control(&agent_control_dir(root, &agent.name), &agent.name)?;
        for cancellation in agent.cancellations {
            record_child_result_to_parent_context(
                &cancellation.parent_session,
                &cancellation.child,
                ChildContextStatus::Cancelled,
                &format!(
                    "Child agent `{}` cancelled because the parent agent stopped.\n",
                    agent.name
                ),
                "",
            )
            .map_err(schedule_child_context_cli_error)?;
        }
        if let Some(cleanup) = agent.cleanup {
            execute_temp_cleanup(cleanup)?;
        }
    }
    Ok(())
}

fn plan_temp_cleanup(root: &Path, name: &str) -> Result<TempCleanupPlan, CliError> {
    let agent_root = root.join("agent");
    preflight_cleanup_directory(&agent_root)?;

    let mut entries = Vec::new();
    for path in [
        agent_object_path(root, name),
        agent_socket_path(root, name)?,
    ] {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_dir() => {
                return Err(CliError::unavailable(format!(
                    "temp agent path is not a file or socket: {}",
                    path.display()
                )));
            }
            Ok(_metadata) => entries.push(TempCleanupEntry {
                path,
                directory: false,
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot stat {}: {error}",
                    path.display()
                )));
            }
        }
    }
    plan_temp_cleanup_tree(&agent_control_dir(root, name), &mut entries)?;
    Ok(TempCleanupPlan { entries })
}

fn plan_temp_cleanup_tree(
    directory: &Path,
    entries: &mut Vec<TempCleanupEntry>,
) -> Result<(), CliError> {
    preflight_cleanup_directory(directory)?;
    for name in read_dir_names(directory)? {
        let path = directory.join(name);
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
        })?;
        if metadata.file_type().is_dir() {
            plan_temp_cleanup_tree(&path, entries)?;
        } else {
            entries.push(TempCleanupEntry {
                path,
                directory: false,
            });
        }
    }
    entries.push(TempCleanupEntry {
        path: directory.to_path_buf(),
        directory: true,
    });
    Ok(())
}

fn preflight_cleanup_directory(path: &Path) -> Result<(), CliError> {
    let directory = open_plain_directory(path).map_err(|error| {
        CliError::unavailable(format!("cannot open {}: {error}", path.display()))
    })?;
    let metadata = directory.metadata().map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
    })?;
    let uid = nix::unistd::Uid::effective().as_raw();
    if uid != 0 && (metadata.uid() != uid || metadata.permissions().mode() & 0o300 != 0o300) {
        return Err(CliError::unavailable(format!(
            "temp cleanup directory is not owner-writable: {}",
            path.display()
        )));
    }
    Ok(())
}

fn execute_temp_cleanup(plan: TempCleanupPlan) -> Result<(), CliError> {
    for entry in plan.entries {
        let result = if entry.directory {
            fs::remove_dir(&entry.path)
        } else {
            fs::remove_file(&entry.path)
        };
        result.map_err(|error| {
            CliError::unavailable(format!("cannot remove {}: {error}", entry.path.display()))
        })?;
    }
    Ok(())
}

pub(crate) fn remove_temp_agent_object(root: &Path, child: &str) -> Result<(), CliError> {
    execute_temp_cleanup(plan_temp_cleanup(root, child)?)
}

fn preflight_stop_control(control: &Path) -> Result<(), CliError> {
    open_plain_directory(control).map_err(|error| {
        CliError::unavailable(format!("cannot open {}: {error}", control.display()))
    })?;
    for file in ["status", "pid"] {
        let path = control.join(file);
        if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(CliError::unavailable(format!(
                "refusing symlink control file: {}",
                path.display()
            )));
        }
        preflight_writable_plain_file(&path)?;
    }
    let log = control.join("log");
    if fs::symlink_metadata(&log).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(CliError::unavailable(format!(
            "refusing symlink log file: {}",
            log.display()
        )));
    }
    preflight_writable_plain_file(&log)?;
    Ok(())
}

fn preflight_writable_plain_file(path: &Path) -> Result<(), CliError> {
    let Some(parent) = path.parent() else {
        return Err(CliError::unavailable("stop path has no parent"));
    };
    let directory = open_plain_directory(parent).map_err(|error| {
        CliError::unavailable(format!("cannot open {}: {error}", parent.display()))
    })?;
    let Some(file) = path.file_name().and_then(|file| file.to_str()) else {
        return Err(CliError::unavailable(format!(
            "invalid stop path: {}",
            path.display()
        )));
    };
    let fd = nix::fcntl::openat(
        &directory,
        file,
        nix::fcntl::OFlag::O_WRONLY | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|error| CliError::unavailable(format!("cannot write {}: {error}", path.display())))?;
    let opened = fs::File::from(fd);
    let metadata = opened.metadata().map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
    })?;
    if !metadata.is_file() {
        return Err(CliError::unavailable(format!(
            "stop path is not a plain file: {}",
            path.display()
        )));
    }
    Ok(())
}

fn plan_parent_child_cancellations(
    root: &Path,
    child_agent: &str,
    parent: &AgentParentRef,
) -> Result<Vec<PlannedCancellation>, CliError> {
    let parent_session_dirs = if let Some(session) = parent.session.as_deref() {
        vec![agent_session_dir(root, &parent.agent, Some(session))?]
    } else {
        let session_root = ctx_home(root)?
            .join("agent")
            .join(&parent.agent)
            .join("session");
        if !session_root
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.is_dir())
        {
            return Ok(Vec::new());
        }
        read_dir_names(&session_root)?
            .into_iter()
            .filter(|name| is_object_name(name))
            .map(|name| session_root.join(name))
            .collect()
    };
    let mut cancellations = Vec::new();
    for parent_session_dir in parent_session_dirs {
        let child_root = parent_session_dir.join("context").join("child");
        if !child_root
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.is_dir())
        {
            continue;
        }
        for child in read_dir_names(&child_root)? {
            let dir = child_root.join(&child);
            if !dir
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.is_dir())
            {
                continue;
            }
            let agent = read_optional_trimmed(&dir.join("agent"))?;
            if agent.as_deref() != Some(child_agent) {
                continue;
            }
            let status = read_optional_trimmed(&dir.join("status"))?.unwrap_or_default();
            if !matches!(
                ChildContextStatus::parse(&status),
                Some(ChildContextStatus::Pending | ChildContextStatus::Active)
            ) {
                continue;
            }
            preflight_child_cancellation(&dir)?;
            cancellations.push(PlannedCancellation {
                parent_session: parent_session_dir.clone(),
                child,
            });
        }
    }
    Ok(cancellations)
}

fn preflight_child_cancellation(child: &Path) -> Result<(), CliError> {
    open_plain_directory(child).map_err(|error| {
        CliError::unavailable(format!("cannot open {}: {error}", child.display()))
    })?;
    for file in CHILD_RESULT_REQUIRED_FILES {
        let path = child.join(file);
        let opened = open_plain_read_file(&path)?;
        let metadata = opened.metadata().map_err(|error| {
            CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
        })?;
        if !metadata.is_file() {
            return Err(CliError::unavailable(format!(
                "child result path is not a plain file: {}",
                path.display()
            )));
        }
    }
    for directory in CHILD_RESULT_REQUIRED_DIRS {
        open_plain_directory(&child.join(directory)).map_err(|error| {
            CliError::unavailable(format!(
                "cannot open {}: {error}",
                child.join(directory).display()
            ))
        })?;
    }
    for file in ["status", "result.md", "refs.jsonl"] {
        preflight_writable_plain_file(&child.join(file))?;
    }
    Ok(())
}

pub(crate) fn agent_stop_log_event(name: &str) -> String {
    format!(
        r#"{{"type":"agent.stop","agent":{},"status":"cancelled"}}"#,
        json_string(name)
    )
}

pub(crate) fn write_agent_control_plain(path: &Path, content: &str) -> Result<(), CliError> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && metadata.file_type().is_symlink()
    {
        return Err(CliError::unavailable(format!(
            "refusing symlink control file: {}",
            path.display()
        )));
    }
    fs::write(path, content)
        .map_err(|error| CliError::unavailable(format!("cannot write {}: {error}", path.display())))
}

pub(crate) fn append_agent_log_event(path: &Path, event: &str) -> Result<(), CliError> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && metadata.file_type().is_symlink()
    {
        return Err(CliError::unavailable(format!(
            "refusing symlink log file: {}",
            path.display()
        )));
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| {
            CliError::unavailable(format!("cannot open {}: {error}", path.display()))
        })?;
    writeln!(file, "{event}")
        .map_err(|error| CliError::unavailable(format!("cannot write {}: {error}", path.display())))
}

pub(crate) fn agent_lifecycle_tool(
    root: &Path,
    name: &str,
    request: &str,
) -> Result<ExitCode, CliError> {
    let Some(hit) = ctx_tool_path(root)?.find(name).map_err(tool_path_error)? else {
        return Err(CliError::unavailable(format!(
            "agent lifecycle tool is not available: tool/{name}"
        )));
    };
    let executable = open_executable_no_follow(hit.path())?;
    let status = agent_lifecycle_tool_command(root, &proc_fd_path(&executable))
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

pub(crate) fn agent_lifecycle_tool_exists(root: &Path, name: &str) -> Result<bool, CliError> {
    Ok(ctx_tool_path(root)?
        .find(name)
        .map_err(tool_path_error)?
        .is_some())
}

pub(crate) fn agent_lifecycle_tool_command(root: &Path, path: &Path) -> ProcessCommand {
    let mut command = ProcessCommand::new(path);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("CTX_ROOT", root);
    command
}

pub(crate) fn agent_name_request_json(name: &str) -> String {
    format!("{{\"name\":{}}}", json_string(name))
}
