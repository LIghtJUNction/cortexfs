use crate::*;

const DEFAULT_AGENT: &str = "coder";

pub(crate) fn start_default_session(root: &Path) -> Result<ExitCode, CliError> {
    let agent = configured_default_agent()?;
    let workspace = absolute_existing_path(&env::current_dir().map_err(|error| {
        CliError::unavailable(format!("cannot read current directory: {error}"))
    })?)
    .map_err(|error| CliError::unavailable(format!("cannot resolve current directory: {error}")))?;
    ensure_default_workspace_mount(root, &agent, &workspace)?;
    let request = request_id()?;
    let suffix = request.strip_prefix("ctx-").unwrap_or(&request);
    let session = format!("ctx_{suffix}");
    let args = AgentStartArgs {
        name: agent.clone(),
        session: session.clone(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: Vec::new(),
    };
    agent_start_host(root, &args)?;
    agent_chat(root, &agent, Some(&session), false, &[])
}

pub(crate) fn resume_current_session(
    root: &Path,
    agent: Option<&str>,
    session: Option<&str>,
) -> Result<ExitCode, CliError> {
    let agent = agent.map_or(configured_default_agent()?, str::to_owned);
    require_cli_name("agent name", &agent)?;
    let session = if let Some(session) = session {
        session.to_owned()
    } else {
        let current = absolute_existing_path(&env::current_dir().map_err(|error| {
            CliError::unavailable(format!("cannot read current directory: {error}"))
        })?)
        .map_err(|error| {
            CliError::unavailable(format!("cannot resolve current directory: {error}"))
        })?;
        session_for_workspace(root, &agent, &current)?.ok_or_else(|| {
            CliError::unavailable(format!(
                "no session for agent {agent} in {}; pass --session SESSION",
                current.display()
            ))
        })?
    };
    require_cli_name("session name", &session)?;
    agent_resume(root, &agent, Some(&session), false)
}

fn configured_default_agent() -> Result<String, CliError> {
    let value = env::var("CTX_DEFAULT_AGENT")
        .ok()
        .or_else(|| env::var("CTX_AGENT").ok())
        .unwrap_or_else(|| DEFAULT_AGENT.to_owned());
    require_cli_name("default agent", &value)?;
    Ok(value)
}

fn session_for_workspace(
    root: &Path,
    agent: &str,
    workspace: &Path,
) -> Result<Option<String>, CliError> {
    let session_root = cortexfs_paths::agent_sessions_from_home_path(&ctx_home(root)?, agent);
    let current = current_session_name(&session_root).ok();
    let mut matches = Vec::new();
    let entries = match fs::read_dir(&session_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(CliError::unavailable(format!(
                "cannot read {}: {error}",
                session_root.display()
            )));
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name == "index" || !plain_session_dir_exists(&path) {
            continue;
        }
        let Some(value) = read_optional_trimmed(&path.join("workspace"))? else {
            continue;
        };
        if workspace_matches(&value, workspace) {
            if current.as_deref() == Some(name.as_str()) {
                return Ok(Some(name));
            }
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(UNIX_EPOCH);
            matches.push((modified, name));
        }
    }
    matches.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    Ok(matches.pop().map(|(_, name)| name))
}

fn workspace_matches(stored: &str, workspace: &Path) -> bool {
    let stored = Path::new(stored);
    stored == workspace || fs::canonicalize(stored).ok().as_deref() == Some(workspace)
}
