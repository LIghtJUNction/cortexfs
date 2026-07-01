fn agent_children(root: &Path, name: &str, session: Option<&str>) -> Result<(), CliError> {
    for row in agent_child_rows(root, name, session)? {
        print_line(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            terminal_safe_text(&row.child),
            terminal_safe_text(&row.status),
            terminal_safe_text(&row.agent),
            terminal_safe_text(&row.session),
            terminal_safe_text(row.parent_session.as_deref().unwrap_or("-")),
            terminal_safe_text(&row.model),
            terminal_safe_text(&row.life),
            terminal_safe_text(&row.agent_status),
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
    let agent =
        read_optional_trimmed(&child_dir.join("agent"))?.unwrap_or_else(|| "agent?".to_owned());
    let session =
        read_optional_trimmed(&child_dir.join("session"))?.unwrap_or_else(|| "default".to_owned());
    let control = root.join("agent").join(format!("{agent}.d"));
    let (model, life) = if is_object_name(&agent) {
        (
            read_agent_control_trimmed(&control, "model")?
                .unwrap_or_else(|| default_agent_process_model(&agent).to_owned()),
            agent_life_for_display(&control)?,
        )
    } else {
        ("unknown".to_owned(), "unknown".to_owned())
    };
    let result = read_file_to_string(&child_dir.join("result.md"))?;
    print_line(&format!(
        "{}\t{}\t{}\t{}\t{}\t{}",
        terminal_safe_text(child),
        status.as_str(),
        terminal_safe_text(&agent),
        terminal_safe_text(&session),
        terminal_safe_text(&model),
        terminal_safe_text(&life)
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
    let child_agent = read_optional_trimmed(&child_dir.join("agent"))?;
    let child_session = read_optional_trimmed(&child_dir.join("session"))?;
    let Some(child_agent) = child_agent.as_deref() else {
        return Ok(Some(status));
    };
    let Some(child_session) = child_session.as_deref() else {
        return Ok(Some(status));
    };
    let control = root.join("agent").join(format!("{child_agent}.d"));
    let Some(parent_ref) = read_agent_parent_ref(&control)? else {
        return Ok(Some(status));
    };
    let parent_session = parent_session_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let parent_session_matches = parent_ref
        .session
        .as_deref()
        .is_none_or(|session| session == parent_session);
    let (agent_status, agent_pid) = live_agent_status_and_pid(&control)?;
    if parent_ref.agent != parent_agent || !parent_session_matches || agent_status != "dead" {
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
    model: String,
    life: String,
    agent_status: String,
    pid: Option<String>,
}

fn agent_child_rows(
    root: &Path,
    name: &str,
    session: Option<&str>,
) -> Result<Vec<AgentChildRow>, CliError> {
    let parent_session_dir = agent_session_dir(root, name, session)?;
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
        let agent = read_optional_trimmed(&dir.join("agent"))?.unwrap_or_else(|| "agent?".to_owned());
        let session = read_optional_trimmed(&dir.join("session"))?.unwrap_or_else(|| "default".to_owned());
        let control = root.join("agent").join(format!("{agent}.d"));
        let (agent_status, pid, model, life, parent_session) = if is_object_name(&agent) {
            let parent = read_agent_parent_ref(&control)?;
            let (agent_status, pid) = live_agent_status_and_pid(&control)?;
            (
                agent_status,
                pid,
                read_agent_control_trimmed(&control, "model")?
                    .unwrap_or_else(|| default_agent_process_model(&agent).to_owned()),
                agent_life_for_display(&control)?,
                parent.and_then(|parent| parent.session),
            )
        } else {
            (
                "unknown".to_owned(),
                None,
                "unknown".to_owned(),
                "unknown".to_owned(),
                None,
            )
        };
        rows.push(AgentChildRow {
            child,
            status,
            agent,
            session,
            parent_session,
            model,
            life,
            agent_status,
            pid,
        });
    }
    Ok(rows)
}

fn agent_life_for_display(control: &Path) -> Result<String, CliError> {
    match fs::symlink_metadata(control) {
        Ok(_metadata) => {
            Ok(read_agent_control_trimmed(control, "life")?.unwrap_or_else(|| "owned".to_owned()))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok("unknown".to_owned()),
        Err(error) => Err(CliError::unavailable(format!(
            "cannot stat {}: {error}",
            control.display()
        ))),
    }
}
