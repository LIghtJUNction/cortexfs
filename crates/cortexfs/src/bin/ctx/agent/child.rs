fn agent_children(root: &Path, name: &str, session: Option<&str>) -> Result<(), CliError> {
    for row in agent_child_rows(root, name, session)? {
        print_line(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            terminal_safe_text(&row.child),
            terminal_safe_text(&row.status),
            terminal_safe_text(&row.agent),
            terminal_safe_text(&row.session),
            terminal_safe_text(row.parent_session.as_deref().unwrap_or("-")),
            terminal_safe_text(row.parent_run.as_deref().unwrap_or("-")),
            terminal_safe_text(&row.model),
            terminal_safe_text(&row.life),
            agent_role_for_display(&row.agent),
            terminal_safe_text(&row.agent_status),
            terminal_safe_text(row.ppid.as_deref().unwrap_or("-")),
            terminal_safe_text(row.pid.as_deref().unwrap_or("-"))
        ))?;
    }
    Ok(())
}

fn agent_wait(
    root: &Path,
    name: &str,
    session: Option<&str>,
    child: &str,
) -> Result<ExitCode, CliError> {
    require_cli_name("agent name", name)?;
    require_cli_name("child name", child)?;
    let parent_session_dir = agent_session_dir(root, name, session)?;
    let child_dir = parent_session_dir.join("context").join("child").join(child);
    let status = reconcile_active_child_wait(root, name, &parent_session_dir, child, &child_dir)?
        .ok_or_else(|| CliError::unavailable(format!("missing child status: {child}")))?;
    let status = ChildContextStatus::parse(&status)
        .ok_or_else(|| CliError::usage(format!("invalid child status for {child}: {status}")))?;
    if matches!(status, ChildContextStatus::Pending | ChildContextStatus::Active) {
        return Err(CliError::unavailable(format!(
            "child {child} is not terminal: {}",
            status.as_str()
        )));
    }
    let (agent, session) = schedule_child_context_agent_session(&child_dir)?;
    let control = agent_control_dir(root, &agent);
    let parent = read_agent_parent_ref(&control)?;
    require_child_backing_parent(name, &parent_session_dir, child, &agent, parent.as_ref())?;
    let model = read_agent_model_for_context(&control, "agent")?;
    let life = agent_life_for_display(&control)?;
    let result = read_file_to_string(&child_dir.join("result.md"))?;
    if status == ChildContextStatus::Cancelled
        && life == "temp"
        && is_object_name(&agent)
        && is_dedicated_worker_agent_name(&agent)
    {
        remove_temp_agent_object(root, &agent)?;
    }
    print_line(&format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
        terminal_safe_text(child),
        status.as_str(),
        terminal_safe_text(&agent),
        terminal_safe_text(&session),
        terminal_safe_text(&model),
        terminal_safe_text(&life),
        agent_role_for_display(&agent)
    ))?;
    print_terminal_text(&result)?;
    Ok(child_wait_exit_code(status))
}

fn reconcile_active_child_wait(
    root: &Path,
    parent_agent: &str,
    parent_session_dir: &Path,
    child: &str,
    child_dir: &Path,
) -> Result<Option<String>, CliError> {
    let Some(status) = read_optional_trimmed(&child_dir.join("status"))? else {
        return Ok(None);
    };
    if ChildContextStatus::parse(&status) != Some(ChildContextStatus::Active) {
        return Ok(Some(status));
    }
    let (child_agent, child_session) = schedule_child_context_agent_session(child_dir)?;
    let control = agent_control_dir(root, &child_agent);
    let Some(parent_ref) = read_agent_parent_ref(&control)? else {
        return Ok(Some(status));
    };
    let parent_session = schedule_parent_session_for_output(parent_session_dir)?;
    let (agent_status, agent_pid) = live_agent_status_and_pid(&control)?;
    if !agent_parent_ref_matches(&parent_ref, parent_agent, Some(&parent_session), None)
        || agent_status != "dead"
    {
        return Ok(Some(status));
    }
    if agent_pid.is_some() {
        return Ok(Some(status));
    }
    record_child_result_to_parent_context(
        parent_session_dir,
        child,
        ChildContextStatus::Cancelled,
        &format!("Child agent `{child_agent}` session `{child_session}` is dead.\n"),
        "",
    )
    .map_err(schedule_child_context_cli_error)?;
    Ok(Some(ChildContextStatus::Cancelled.as_str().to_owned()))
}

fn child_wait_exit_code(status: ChildContextStatus) -> ExitCode {
    ExitCode::from(match status {
        ChildContextStatus::Done => 0,
        ChildContextStatus::Error => 1,
        ChildContextStatus::Cancelled => 130,
        ChildContextStatus::Pending | ChildContextStatus::Active => 69,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AgentChildRow {
    child: String,
    status: String,
    agent: String,
    session: String,
    parent_session: Option<String>,
    parent_run: Option<String>,
    model: String,
    life: String,
    agent_status: String,
    ppid: Option<String>,
    pid: Option<String>,
}

fn agent_child_rows(
    root: &Path,
    name: &str,
    session: Option<&str>,
) -> Result<Vec<AgentChildRow>, CliError> {
    let parent_session_dir = agent_session_dir(root, name, session)?;
    let parent_pid = agent_live_pid(root, name)?;
    let child_root = parent_session_dir.join("context").join("child");
    if !child_root
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.is_dir())
    {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    for child in read_dir_names(&child_root)? {
        let dir = child_root.join(&child);
        if !dir
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.is_dir())
        {
            continue;
        }
        let status = reconcile_active_child_wait(root, name, &parent_session_dir, &child, &dir)?
            .unwrap_or_else(|| "unknown".to_owned());
        let (agent, session) = schedule_child_context_agent_session(&dir)?;
        let control = agent_control_dir(root, &agent);
        let parent = read_agent_parent_ref(&control)?;
        let parent_ref =
            require_child_backing_parent(name, &parent_session_dir, &child, &agent, parent.as_ref())?;
        let (agent_status, pid) = live_agent_status_and_pid(&control)?;
        let model = read_agent_model_for_context(&control, "agent")?;
        let life = agent_life_for_display(&control)?;
        rows.push(AgentChildRow {
            child,
            status,
            agent,
            session,
            parent_session: parent_ref.as_ref().and_then(|parent| parent.session.clone()),
            parent_run: parent_ref.and_then(|parent| parent.run),
            model,
            life,
            agent_status,
            ppid: parent_pid.clone(),
            pid,
        });
    }
    Ok(rows)
}

fn require_child_backing_parent(
    parent_agent: &str,
    parent_session_dir: &Path,
    child: &str,
    child_agent: &str,
    parent: Option<&AgentParentRef>,
) -> Result<Option<AgentParentRef>, CliError> {
    let Some(parent) = parent else {
        return Ok(None);
    };
    let parent_session = schedule_parent_session_for_output(parent_session_dir)?;
    if !agent_parent_ref_matches(parent, parent_agent, Some(&parent_session), None) {
        return Err(CliError::usage(format!(
            "child {child} backing parent mismatch for {child_agent}: {}",
            agent_parent_ref_display(parent)
        )));
    }
    Ok(Some(parent.clone()))
}

fn agent_life_for_display(control: &Path) -> Result<String, CliError> {
    match fs::symlink_metadata(control) {
        Ok(_metadata) => read_agent_life_for_context(control, "agent"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok("unknown".to_owned()),
        Err(error) => Err(CliError::unavailable(format!(
            "cannot stat {}: {error}",
            control.display()
        ))),
    }
}
