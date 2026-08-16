use crate::*;

pub(crate) fn agent_tools(root: &Path, name: &str) -> Result<(), CliError> {
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
pub(crate) struct AgentVisibleTool {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) status: String,
}

pub(crate) fn agent_visible_tool_entries(
    root: &Path,
    name: &str,
) -> Result<Vec<AgentVisibleTool>, CliError> {
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
            let status_path = tool_path
                .parent()
                .map(|parent| cortexfs_paths::tool_control_file_path(parent, &tool, "status"))
                .ok_or_else(|| CliError::unavailable("tool path has no parent"))?;
            let status =
                read_optional_trimmed(&status_path)?.unwrap_or_else(|| "unknown".to_owned());
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

pub(crate) fn is_control_or_socket_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("sock") || ext.eq_ignore_ascii_case("d"))
}

pub(crate) fn agent_cwd(root: &Path, name: &str) -> Result<String, CliError> {
    let path = agent_control_dir(root, name).join("cwd");
    Ok(read_optional_trimmed(&path)?.unwrap_or_else(|| "/workspace".to_owned()))
}

pub(crate) fn latest_run_id(root: &Path, name: &str, session: &str) -> Result<String, CliError> {
    let session_dir = agent_session_dir(root, name, Some(session))?;
    if let Some(run) = read_optional_trimmed(&session_dir.join("current_run"))? {
        return Ok(run);
    }
    let events = session_dir.join("events.jsonl");
    let max_bytes = usize::try_from(MAX_CTX_FILE_CHECK_BYTES).map_err(|error| {
        CliError::unavailable(format!("cannot read {}: {error}", events.display()))
    })?;
    let bytes =
        columnar::tail(&session_dir, columnar::Stream::Events, max_bytes).map_err(|error| {
            CliError::unavailable(format!("cannot read {}: {error}", events.display()))
        })?;
    let content = String::from_utf8(bytes).map_err(|error| {
        CliError::unavailable(format!("cannot read {}: {error}", events.display()))
    })?;
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

pub(crate) fn read_optional_trimmed(path: &Path) -> Result<Option<String>, CliError> {
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
