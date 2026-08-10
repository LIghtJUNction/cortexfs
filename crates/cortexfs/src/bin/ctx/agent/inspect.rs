use crate::*;

pub(crate) fn agent_inspect(
    root: &Path,
    name: &str,
    session: Option<&str>,
) -> Result<(), CliError> {
    for line in agent_inspect_lines(root, name, session)? {
        print_line(&line)?;
    }
    Ok(())
}

pub(crate) fn agent_inspect_lines(
    root: &Path,
    name: &str,
    session: Option<&str>,
) -> Result<Vec<String>, CliError> {
    require_cli_name("agent name", name)?;
    let view = derive_agent_runtime_view(root, name)
        .map_err(|error| CliError::unavailable(format!("agent view {}: {name}", error.errno())))?;
    let control = agent_control_dir(root, name);
    let session_name = agent_session_name(root, name, session)?;
    let session_dir = agent_session_dir(root, name, Some(&session_name))?;
    let receipt = read_optional_trimmed(&control.join("meta.json"))?
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|meta| meta.get("runtime_receipt").cloned())
        .is_some();
    let model_control = inspect_model_control(root, view.model());
    let capabilities = model_control
        .as_ref()
        .map(|path| path.join("cap"))
        .map(|path| read_optional_trimmed(&path))
        .transpose()?
        .flatten()
        .map_or_else(
            || "-".to_owned(),
            |value| value.lines().collect::<Vec<_>>().join(","),
        );
    let tools = agent_visible_tool_entries(root, name)?;
    Ok(vec![
        format!("definition={}", root.join("agent").join(name).display()),
        format!("control={}", control.display()),
        format!(
            "instance.status={}",
            inspect_value(&control.join("status"))?
        ),
        format!("instance.pid={}", inspect_value(&control.join("pid"))?),
        format!("instance.receipt={}", if receipt { "present" } else { "-" }),
        format!("session.name={session_name}"),
        format!("session.path={}", session_dir.display()),
        format!(
            "session.state={}",
            inspect_value(&session_dir.join("state"))?
        ),
        format!("session.cwd={}", inspect_value(&session_dir.join("cwd"))?),
        format!("model.name={}", view.model()),
        format!("model.limit={}", view.model_limit()),
        format!("model.cap={capabilities}"),
        format!("policy={}", control.join("policy").display()),
        format!("mount={}", control.join("mount").display()),
        format!("tools={}", tools.len()),
    ])
}

fn inspect_value(path: &Path) -> Result<String, CliError> {
    Ok(read_optional_trimmed(path)?.unwrap_or_else(|| "-".to_owned()))
}

fn inspect_model_control(root: &Path, model: &str) -> Option<PathBuf> {
    let model = if is_model_name(model) {
        model.to_owned()
    } else {
        let target = fs::read_link(root.join("model").join(model)).ok()?;
        let target = target.to_str()?;
        target
            .strip_prefix("/ctx/model/")
            .or_else(|| target.strip_prefix("model/"))
            .filter(|target| is_model_name(target))?
            .to_owned()
    };
    let (provider, name) = model.split_once('/')?;
    Some(root.join("model").join(provider).join(format!("{name}.d")))
}
