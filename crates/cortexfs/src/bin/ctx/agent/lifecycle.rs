fn agent_stop_host_fallback(root: &Path, name: &str) -> Result<ExitCode, CliError> {
    let control = agent_control_dir(root, name);
    stop_agent_terminal_units(root, name)?;
    stop_agent_control(&control, name)?;
    stop_owned_child_agents(root, name)?;
    print_line(&format!("agent {} stopped", terminal_safe_text(name)))?;
    Ok(ExitCode::SUCCESS)
}

fn stop_agent_terminal_units(root: &Path, name: &str) -> Result<(), CliError> {
    for unit in agent_terminal_units(root, name)? {
        reset_agent_terminal_unit(&unit);
    }
    Ok(())
}

fn agent_terminal_units(root: &Path, name: &str) -> Result<Vec<String>, CliError> {
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

fn stop_agent_control(control: &Path, name: &str) -> Result<(), CliError> {
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

fn stop_owned_child_agents(root: &Path, parent: &str) -> Result<(), CliError> {
    let mut children = Vec::new();
    for (name, control) in agent_control_dirs(root)? {
        if name == parent {
            continue;
        }
        let Some(child_parent) = read_agent_parent_ref(&control)? else {
            continue;
        };
        if child_parent.agent == parent && agent_lifecycle_is_parent_owned(&control)? {
            children.push((name, child_parent, agent_lifecycle_is_temp(&control)?));
        }
    }
    children.sort_by(|left, right| left.0.cmp(&right.0));
    for (child, child_parent, is_temp) in children {
        let control = agent_control_dir(root, &child);
        stop_agent_terminal_units(root, &child)?;
        stop_agent_control(&control, &child)?;
        record_parent_child_cancellation(root, &child, &child_parent)?;
        stop_owned_child_agents(root, &child)?;
        if is_temp && is_dedicated_worker_agent_name(&child) {
            remove_temp_agent_object(root, &child)?;
        }
    }
    Ok(())
}

fn remove_temp_agent_object(root: &Path, child: &str) -> Result<(), CliError> {
    remove_temp_agent_file(&agent_object_path(root, child))?;
    remove_temp_agent_file(&agent_socket_path(root, child)?)?;
    let control = agent_control_dir(root, child);
    match fs::symlink_metadata(&control) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(&control).map_err(|error| {
                CliError::unavailable(format!("cannot remove {}: {error}", control.display()))
            })
        }
        Ok(_metadata) => Err(CliError::unavailable(format!(
            "temp agent control is not a plain directory: {}",
            control.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CliError::unavailable(format!(
            "cannot stat {}: {error}",
            control.display()
        ))),
    }
}

fn remove_temp_agent_file(path: &Path) -> Result<(), CliError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_dir() => {
            fs::remove_file(path).map_err(|error| {
                CliError::unavailable(format!("cannot remove {}: {error}", path.display()))
            })
        }
        Ok(_metadata) => Err(CliError::unavailable(format!(
            "temp agent path is not a file or socket: {}",
            path.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CliError::unavailable(format!(
            "cannot stat {}: {error}",
            path.display()
        ))),
    }
}

fn record_parent_child_cancellation(
    root: &Path,
    child_agent: &str,
    parent: &AgentParentRef,
) -> Result<(), CliError> {
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
            return Ok(());
        }
        read_dir_names(&session_root)?
            .into_iter()
            .filter(|name| is_object_name(name))
            .map(|name| session_root.join(name))
            .collect()
    };
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
            if !matches!(ChildContextStatus::parse(&status), Some(ChildContextStatus::Pending | ChildContextStatus::Active)) {
                continue;
            }
            record_child_result_to_parent_context(
                &parent_session_dir,
                &child,
                ChildContextStatus::Cancelled,
                &format!("Child agent `{child_agent}` cancelled because the parent agent stopped.\n"),
                "",
            )
            .map_err(schedule_child_context_cli_error)?;
        }
    }
    Ok(())
}

fn agent_lifecycle_is_parent_owned(control: &Path) -> Result<bool, CliError> {
    Ok(matches!(
        read_agent_control_trimmed(control, "life")?.as_deref(),
        None | Some("owned" | "temp")
    ))
}

fn agent_lifecycle_is_temp(control: &Path) -> Result<bool, CliError> {
    Ok(matches!(
        read_agent_control_trimmed(control, "life")?.as_deref(),
        Some("temp")
    ))
}

fn agent_stop_log_event(name: &str) -> String {
    format!(
        r#"{{"type":"agent.stop","agent":{},"status":"cancelled"}}"#,
        json_string(name)
    )
}

fn write_agent_control_plain(path: &Path, content: &str) -> Result<(), CliError> {
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

fn append_agent_log_event(path: &Path, event: &str) -> Result<(), CliError> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && metadata.file_type().is_symlink()
    {
        return Err(CliError::unavailable(format!(
            "refusing symlink log file: {}",
            path.display()
        )));
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| CliError::unavailable(format!("cannot open {}: {error}", path.display())))?;
    writeln!(file, "{event}")
        .map_err(|error| CliError::unavailable(format!("cannot write {}: {error}", path.display())))
}

fn agent_lifecycle_tool(root: &Path, name: &str, request: &str) -> Result<ExitCode, CliError> {
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

fn agent_lifecycle_tool_exists(root: &Path, name: &str) -> Result<bool, CliError> {
    Ok(ctx_tool_path(root)?
        .find(name)
        .map_err(tool_path_error)?
        .is_some())
}

fn agent_lifecycle_tool_command(root: &Path, path: &Path) -> ProcessCommand {
    let mut command = ProcessCommand::new(path);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("CTX_ROOT", root);
    command
}

fn agent_name_request_json(name: &str) -> String {
    format!("{{\"name\":{}}}", json_string(name))
}
