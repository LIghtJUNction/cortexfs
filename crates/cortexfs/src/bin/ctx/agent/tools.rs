fn agent_tools(root: &Path, name: &str) -> Result<(), CliError> {
    for entry in agent_visible_tool_entries(root, name)? {
        print_line(&format!(
            "{}\t{}\t{}",
            terminal_safe_text(&entry.name),
            terminal_safe_text(&entry.path.display().to_string()),
            terminal_safe_text(&entry.status)
        ))?;
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AgentVisibleTool {
    name: String,
    path: PathBuf,
    status: String,
}

fn agent_native_tool_names(root: &Path, name: &str) -> Result<Vec<String>, CliError> {
    require_cli_name("agent name", name)?;
    let view = derive_agent_runtime_view(root, name)
        .map_err(|error| CliError::unavailable(format!("agent view {}: {name}", error.errno())))?;
    if !agent_tool_is_authorized(&view, "tsh")? {
        return Ok(Vec::new());
    }
    let mut tools = vec!["tsh".to_owned()];
    let state_path = cortexfs::tsh_context_state_path(view.home());
    let state = cortexfs::read_tsh_context_state(&state_path).map_err(|error| {
        CliError::unavailable(format!("cannot read {}: {error}", state_path.display()))
    })?;
    for tool in state.tools {
        if tool.name == "tsh" || tools.contains(&tool.name) {
            continue;
        }
        if agent_tool_is_authorized(&view, &tool.name)? {
            tools.push(tool.name);
        }
    }
    tools.sort();
    tools.dedup();
    Ok(tools)
}

fn agent_tool_is_authorized(
    view: &AgentRuntimeView,
    tool: &str,
) -> Result<bool, CliError> {
    let Some(hit) = view
        .tool_path()
        .find(tool)
        .map_err(|error| CliError::unavailable(format!("cannot inspect CTX_PATH: {error:?}")))?
    else {
        return Ok(false);
    };
    let policy = read_file_to_string(&hit.control_dir().join("policy")).map_err(|error| {
        CliError::unavailable(format!(
            "cannot read {}: {}",
            hit.control_dir().join("policy").display(),
            error.message
        ))
    })?;
    let tool_policy = PolicyV0::parse(&policy)
        .map_err(|_error| CliError::unavailable(format!("invalid policy for tool:{tool}")))?;
    Ok(authorize_tool_execution(
        view.tool_path(),
        tool,
        ToolExecutionAuthority::new(
            view.identity(),
            view.mount_table(),
            view.policy_subject(),
            view.policy(),
            &tool_policy,
        ),
    )
    .is_ok())
}

fn agent_visible_tool_entries(root: &Path, name: &str) -> Result<Vec<AgentVisibleTool>, CliError> {
    require_cli_name("agent name", name)?;
    let mut paths = Vec::new();
    paths.extend(ctx_tool_path(root)?.dirs().iter().map(PathBuf::from));
    let agent_path = agent_control_dir(root, name).join("path");
    if let Ok(content) = read_file_to_string(&agent_path) {
        paths.extend(content.lines().map(PathBuf::from));
    }
    paths.sort();
    paths.dedup();
    let mut tools = Vec::new();
    for path in paths {
        if !path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.is_dir())
        {
            continue;
        }
        for tool in read_dir_names(&path)? {
            let tool_path = path.join(&tool);
            if !is_executable_file(&tool_path) || is_control_or_socket_name(&tool) {
                continue;
            }
            let status = read_optional_trimmed(&tool_path.with_file_name(format!("{tool}.d")).join("status"))?
                .unwrap_or_else(|| "unknown".to_owned());
            tools.push(AgentVisibleTool {
                name: tool,
                path: tool_path,
                status,
            });
        }
    }
    tools.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.path.cmp(&right.path))
    });
    tools.dedup_by(|left, right| left.name == right.name && left.path == right.path);
    Ok(tools)
}

fn is_control_or_socket_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("sock") || ext.eq_ignore_ascii_case("d"))
}

fn agent_cwd(root: &Path, name: &str) -> Result<String, CliError> {
    let path = agent_control_dir(root, name).join("cwd");
    Ok(read_optional_trimmed(&path)?.unwrap_or_else(|| "/workspace".to_owned()))
}

fn latest_run_id(root: &Path, name: &str, session: &str) -> Result<String, CliError> {
    let session_dir = agent_session_dir(root, name, Some(session))?;
    if let Some(run) = read_optional_trimmed(&session_dir.join("current_run"))? {
        return Ok(run);
    }
    let events = session_dir.join("events.jsonl");
    let content = read_file_to_string(&events)?;
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
    match fs::symlink_metadata(path) {
        Ok(_metadata) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(CliError::unavailable(format!(
                "cannot stat {}: {error}",
                path.display()
            )));
        }
    }
    let value = read_file_to_string(path)?;
    let value = value.trim();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value.to_owned()))
    }
}
