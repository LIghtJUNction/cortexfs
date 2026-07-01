fn schedule_command(root: &Path, args: &ScheduleArgs) -> Result<(), CliError> {
    match *args {
        ScheduleArgs::Status {
            ref path,
            ref done,
        } => schedule_status(root, path, done),
        ScheduleArgs::Advance {
            ref path,
            ref done,
        } => schedule_advance(root, path, done),
        ScheduleArgs::Claim {
            ref path,
            ref child,
        } => schedule_claim(root, path, child),
        ScheduleArgs::Result {
            ref path,
            ref child,
            status,
            ref result,
            ref refs_jsonl,
        } => schedule_result(root, path, child, status, result, refs_jsonl),
    }
}

fn schedule_status(root: &Path, path: &str, done: &[String]) -> Result<(), CliError> {
    let schedule = load_schedule_context(root, path, "status")?;
    for line in schedule_status_lines(root, &schedule, done)? {
        print_line(&line)?;
    }
    Ok(())
}

fn schedule_status_lines(
    root: &Path,
    schedule: &LoadedScheduleContext,
    done: &[String],
) -> Result<Vec<String>, CliError> {
    let done_refs = done.iter().map(String::as_str).collect::<Vec<_>>();
    let completed = completed_agent_schedule_nodes_from_parent_context(
        &schedule.parent_session_dir,
        &schedule.json,
        &schedule.parent_subject,
        &schedule.parent_policy,
        &done_refs,
    )
    .map_err(|error| schedule_record_cli_error("status", error))?;
    let completed_refs = completed.iter().map(String::as_str).collect::<Vec<_>>();
    let ready = ready_agent_schedule_nodes(
        &schedule.json,
        &schedule.parent_subject,
        &schedule.parent_policy,
        &completed_refs,
    )
    .map_err(|report| schedule_record_cli_error(
        "status",
        AgentScheduleRecordError::InvalidSchedule(report),
    ))?;
    let ready = ready.iter().map(AgentScheduleNode::id).collect::<HashSet<_>>();
    let completed = completed.iter().map(String::as_str).collect::<HashSet<_>>();
    let mut lines = Vec::new();
    for node in agent_schedule_nodes(
        &schedule.json,
        &schedule.parent_subject,
        &schedule.parent_policy,
    )
    .map_err(|report| schedule_record_cli_error(
        "status",
        AgentScheduleRecordError::InvalidSchedule(report),
    ))? {
        let (model, life) = schedule_node_model_life(root, &node)?;
        lines.push(format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            terminal_safe_text(node.id()),
            node.kind().as_str(),
            terminal_safe_text(node.agent()),
            terminal_safe_text(node.child().unwrap_or("-")),
            terminal_safe_text(&schedule_node_session(&schedule.parent_session_dir, &node)?),
            terminal_safe_text(&model),
            terminal_safe_text(&life),
            node.child().map_or("-", |_| schedule_handoff_agent_role(node.agent())),
            schedule_node_state(root, schedule, &completed, &ready, &node)?,
        ));
    }
    Ok(lines)
}

fn schedule_node_model_life(
    root: &Path,
    node: &AgentScheduleNode,
) -> Result<(String, String), CliError> {
    if node.child().is_some() {
        schedule_handoff_agent_model_life(root, node.agent())
    } else {
        Ok(("-".to_owned(), "-".to_owned()))
    }
}

fn schedule_node_session(
    parent_session_dir: &Path,
    node: &AgentScheduleNode,
) -> Result<String, CliError> {
    if node.child().is_none() {
        return Ok("-".to_owned());
    }
    Ok(node
        .child_session()
        .map_or(schedule_parent_session_for_output(parent_session_dir)?, str::to_owned))
}

struct LoadedScheduleContext {
    abi_path: String,
    context_abi_path: String,
    parent_agent: String,
    parent_session_dir: PathBuf,
    parent_subject: String,
    parent_policy: PolicyV0,
    json: String,
}

fn load_schedule_context(
    root: &Path,
    path: &str,
    command: &str,
) -> Result<LoadedScheduleContext, CliError> {
    let abi_path = classify_input_path(root, path)?;
    let parsed = parse_abi_path(&abi_path);
    if !parsed.is_agent_schedule_plan() {
        return Err(CliError::usage(format!(
            "schedule {command} requires an agent session context/plan.json"
        )));
    }
    let parent_agent = parent_agent_for_session_context_path(&abi_path)
        .ok_or_else(|| CliError::usage(format!("schedule {command} plan must belong to an agent session")))?;
    Ok(LoadedScheduleContext {
        context_abi_path: schedule_context_abi_path(&abi_path, command)?,
        parent_agent: parent_agent.to_owned(),
        parent_session_dir: schedule_parent_session_dir(root, path, command)?,
        parent_subject: parent_policy_subject(root, parent_agent)?,
        parent_policy: parent_agent_policy(root, parent_agent)?,
        json: read_file_to_string(&resolve_abi_path(root, path)?)?,
        abi_path,
    })
}

fn schedule_node_state(
    root: &Path,
    schedule: &LoadedScheduleContext,
    completed: &HashSet<&str>,
    ready: &HashSet<&str>,
    node: &AgentScheduleNode,
) -> Result<String, CliError> {
    if completed.contains(node.id()) {
        return Ok("done".to_owned());
    }
    if let Some(child) = node.child() {
        let child_paths = schedule_child_context_abi_paths(&schedule.context_abi_path, child)?;
        let child_dir = resolve_abi_path(root, &child_paths.status)?
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| CliError::usage(format!("invalid child status path for {child}")))?;
        if let Some(status) = reconcile_active_child_wait(
            root,
            &schedule.parent_agent,
            &schedule.parent_session_dir,
            child,
            &child_dir,
        )? {
            return ChildContextStatus::parse(&status)
                .map(ChildContextStatus::as_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    CliError::usage(format!("invalid child status for {child}: {status}"))
                });
        }
    }
    Ok(if ready.contains(node.id()) {
        "ready".to_owned()
    } else {
        "blocked".to_owned()
    })
}

fn schedule_parent_session_dir(root: &Path, path: &str, command: &str) -> Result<PathBuf, CliError> {
    let abi_path = classify_input_path(root, path)?;
    let parsed = parse_abi_path(&abi_path);
    if !parsed.is_agent_schedule_plan() {
        return Err(CliError::usage(format!(
            "schedule {command} requires an agent session context/plan.json"
        )));
    }
    let plan_path = resolve_abi_path(root, path)?;
    plan_path
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| CliError::usage(format!("schedule {command} requires an agent session plan")))
}

fn schedule_advance(root: &Path, path: &str, done: &[String]) -> Result<(), CliError> {
    let schedule = load_schedule_context(root, path, "advance")?;
    let done = done.iter().map(String::as_str).collect::<Vec<_>>();
    let advance = advance_agent_schedule_from_parent_context(
        &schedule.parent_session_dir,
        &schedule.json,
        &schedule.parent_subject,
        &schedule.parent_policy,
        &done,
    )
    .map_err(|error| schedule_record_cli_error("advance", error))?;

    if advance.completed_nodes().is_empty() && advance.handoffs().is_empty() {
        return print_line("ok");
    }
    for node in advance.completed_nodes() {
        print_line(&format!("completed {node}"))?;
    }
    let parent_agent = parent_agent_for_session_context_path(&schedule.abi_path)
        .ok_or_else(|| CliError::usage("schedule advance plan must belong to an agent session"))?;
    let parent_ref = schedule_parent_ref_for_output(parent_agent, &schedule.parent_session_dir)?;
    for handoff in advance.handoffs() {
        let child_paths =
            schedule_child_context_abi_paths(&schedule.context_abi_path, handoff.child())?;
        let (model, life) = schedule_handoff_agent_model_life(root, handoff.agent())?;
        let child_parent = schedule_handoff_agent_parent(root, handoff.agent())?;
        print_line(&format!(
            "handoff node={} child={} agent={} session={} model={} life={} role={} parent={} child_parent={} plan={} handoff={} result={} refs={}",
            handoff.node(),
            handoff.child(),
            handoff.agent(),
            handoff.session(),
            model,
            life,
            schedule_handoff_agent_role(handoff.agent()),
            shell_quote_arg(&parent_ref),
            shell_quote_arg(&child_parent),
            schedule.abi_path,
            child_paths.handoff,
            child_paths.result,
            child_paths.refs,
        ))?;
    }
    Ok(())
}

fn schedule_handoff_agent_model_life(root: &Path, agent: &str) -> Result<(String, String), CliError> {
    require_schedule_handoff_agent(root, agent)?;
    let control = root.join("agent").join(format!("{agent}.d"));
    let model = read_agent_control_trimmed(&control, "model")?
        .unwrap_or_else(|| default_agent_process_model(agent).to_owned());
    if !(is_model_name(&model) || matches!(model.as_str(), "main" | "helper")) {
        return Err(CliError::usage(format!(
            "invalid handoff agent model for {agent}: {model}"
        )));
    }
    let life = read_agent_control_trimmed(&control, "life")?.unwrap_or_else(|| "owned".to_owned());
    if cortexfs::ChildLifecycle::parse(&life).is_err() {
        return Err(CliError::usage(format!("invalid handoff agent life for {agent}: {life}")));
    }
    Ok((model, life))
}

fn schedule_handoff_agent_parent(root: &Path, agent: &str) -> Result<String, CliError> {
    require_schedule_handoff_agent(root, agent)?;
    let parent = read_agent_control_trimmed(&root.join("agent").join(format!("{agent}.d")), "parent")?
        .unwrap_or_else(|| "-".to_owned());
    if parent != "-" && parse_agent_parent_ref(&parent).is_none() {
        return Err(CliError::usage(format!("invalid handoff agent parent for {agent}: {parent}")));
    }
    Ok(parent)
}

fn schedule_handoff_agent_role(agent: &str) -> &'static str {
    if is_worker_agent_name(agent) { "worker" } else { "agent" }
}

fn require_schedule_handoff_agent(root: &Path, agent: &str) -> Result<(), CliError> {
    let object = root.join("agent").join(agent);
    open_executable_no_follow(&object).map(drop).map_err(|error| {
        CliError::usage(format!(
            "missing handoff agent object {agent}: {}",
            error.message
        ))
    })?;
    let control = root.join("agent").join(format!("{agent}.d"));
    let control_metadata = fs::symlink_metadata(&control).map_err(|error| {
        CliError::usage(format!("missing handoff agent control {agent}: {error}"))
    })?;
    if !control_metadata.file_type().is_dir() || control_metadata.file_type().is_symlink() {
        return Err(CliError::usage(format!(
            "handoff agent control is not a plain directory: {}",
            control.display()
        )));
    }
    Ok(())
}

fn schedule_claim(root: &Path, path: &str, child: &str) -> Result<(), CliError> {
    let abi_path = classify_input_path(root, path)?;
    let parent_agent = parent_agent_for_session_context_path(&abi_path)
        .ok_or_else(|| CliError::usage("schedule claim plan must belong to an agent session"))?;
    let parent_session_dir = schedule_parent_session_dir(root, path, "claim")?;
    let context_abi_path = schedule_context_abi_path(&abi_path, "claim")?;
    let child_paths = schedule_child_context_abi_paths(&context_abi_path, child)?;
    let child_dir = resolve_abi_path(root, &child_paths.status)?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| CliError::usage(format!("invalid child status path for {child}")))?;
    let (agent, session) = schedule_child_context_agent_session(&child_dir)?;
    let (model, life) = schedule_handoff_agent_model_life(root, &agent)?;
    let child_parent = schedule_handoff_agent_parent(root, &agent)?;
    schedule_claim_child_active(root, &child_paths.status)?;
    let parent_ref = schedule_parent_ref_for_output(parent_agent, &parent_session_dir)?;
    print_line(&format!(
        "claim child={child} status=active agent={} session={} model={} life={} role={} parent={} child_parent={} plan={} handoff={} result={} refs={}",
        terminal_safe_text(&agent),
        terminal_safe_text(&session),
        terminal_safe_text(&model),
        terminal_safe_text(&life),
        schedule_handoff_agent_role(&agent),
        shell_quote_arg(&parent_ref),
        shell_quote_arg(&child_parent),
        abi_path,
        child_paths.handoff,
        child_paths.result,
        child_paths.refs,
    ))
}

fn schedule_claim_child_active(root: &Path, status_abi_path: &str) -> Result<(), CliError> {
    let status = read_file_to_string(&resolve_abi_path(root, status_abi_path)?)?;
    match ChildContextStatus::parse(status.trim()) {
        Some(ChildContextStatus::Pending) => file_set(root, status_abi_path, "active"),
        Some(ChildContextStatus::Active) => Ok(()),
        Some(ChildContextStatus::Done | ChildContextStatus::Error | ChildContextStatus::Cancelled)
        | None => Err(CliError::usage(
            "invalid child context: invalid status transition",
        )),
    }
}

fn schedule_child_context_agent_session(child_dir: &Path) -> Result<(String, String), CliError> {
    let agent = read_optional_trimmed(&child_dir.join("agent"))?
        .ok_or_else(|| CliError::usage("invalid child context: invalid agent name"))?;
    let session = read_optional_trimmed(&child_dir.join("session"))?
        .ok_or_else(|| CliError::usage("invalid child context: invalid session name"))?;
    if !is_object_name(&agent) {
        return Err(CliError::usage("invalid child context: invalid agent name"));
    }
    if !is_object_name(&session) {
        return Err(CliError::usage("invalid child context: invalid session name"));
    }
    Ok((agent, session))
}

fn schedule_parent_ref_for_output(
    parent_agent: &str,
    parent_session_dir: &Path,
) -> Result<String, CliError> {
    let session = schedule_parent_session_for_output(parent_session_dir)?;
    Ok(format!("agent:{parent_agent} session:{session}"))
}

fn schedule_parent_session_for_output(parent_session_dir: &Path) -> Result<String, CliError> {
    let session = parent_session_dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| is_object_name(name))
        .ok_or_else(|| CliError::unavailable("cannot derive parent session for schedule"))?;
    Ok(session.to_owned())
}

fn schedule_result(
    root: &Path,
    path: &str,
    child: &str,
    status: ChildContextStatus,
    result: &str,
    refs_jsonl: &str,
) -> Result<(), CliError> {
    let abi_path = classify_input_path(root, path)?;
    let parent_agent = parent_agent_for_session_context_path(&abi_path)
        .ok_or_else(|| CliError::usage("schedule result plan must belong to an agent session"))?;
    let parent_session_dir = schedule_parent_session_dir(root, path, "result")?;
    let context_abi_path = schedule_context_abi_path(&abi_path, "result")?;
    let child_paths = schedule_child_context_abi_paths(&context_abi_path, child)?;
    let child_dir = resolve_abi_path(root, &child_paths.status)?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| CliError::usage(format!("invalid child status path for {child}")))?;
    let (agent, session) = schedule_child_context_agent_session(&child_dir)?;
    let (model, life) = schedule_handoff_agent_model_life(root, &agent)?;
    let child_parent = schedule_handoff_agent_parent(root, &agent)?;
    record_child_result_to_parent_context(&parent_session_dir, child, status, result, refs_jsonl)
        .map_err(schedule_child_context_cli_error)?;
    let parent_ref = schedule_parent_ref_for_output(parent_agent, &parent_session_dir)?;
    print_line(&format!(
        "result child={child} status={} agent={} session={} model={} life={} role={} parent={} child_parent={} plan={} result={} refs={}",
        status.as_str(),
        terminal_safe_text(&agent),
        terminal_safe_text(&session),
        terminal_safe_text(&model),
        terminal_safe_text(&life),
        schedule_handoff_agent_role(&agent),
        shell_quote_arg(&parent_ref),
        shell_quote_arg(&child_parent),
        abi_path,
        child_paths.result,
        child_paths.refs,
    ))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ScheduleChildContextAbiPaths {
    status: String,
    handoff: String,
    result: String,
    refs: String,
}

fn schedule_context_abi_path(abi_path: &str, command: &str) -> Result<String, CliError> {
    abi_path
        .strip_suffix("/plan.json")
        .map(str::to_owned)
        .ok_or_else(|| CliError::usage(format!("schedule {command} requires context/plan.json")))
}

fn schedule_child_context_abi_paths(
    context_abi_path: &str,
    child: &str,
) -> Result<ScheduleChildContextAbiPaths, CliError> {
    if !is_object_name(child) {
        return Err(CliError::usage("invalid child context path: invalid child name"));
    }
    let base = format!("{context_abi_path}/child/{child}");
    Ok(ScheduleChildContextAbiPaths {
        status: format!("{base}/status"),
        handoff: format!("{base}/handoff.md"),
        result: format!("{base}/result.md"),
        refs: format!("{base}/refs.jsonl"),
    })
}

fn schedule_record_cli_error(command: &str, error: AgentScheduleRecordError) -> CliError {
    match error {
        AgentScheduleRecordError::InvalidText => {
            CliError::usage("invalid agent schedule: contains NUL byte")
        }
        AgentScheduleRecordError::InvalidSchedule(report) => CliError::usage(format!(
            "invalid agent schedule: {}",
            format_agent_schedule_issues(report.issues())
        )),
        AgentScheduleRecordError::MissingParentSession => {
            CliError::unavailable(format!("cannot {command} agent schedule: missing parent session"))
        }
        AgentScheduleRecordError::CannotRecord => CliError::unavailable(format!(
            "cannot {command} agent schedule: {}",
            AgentScheduleRecordError::CannotRecord.errno()
        )),
    }
}

fn schedule_child_context_cli_error(error: ChildContextRecordError) -> CliError {
    match error {
        ChildContextRecordError::InvalidChildName => CliError::usage("invalid child context: invalid child name"),
        ChildContextRecordError::InvalidAgentName => CliError::usage("invalid child context: invalid agent name"),
        ChildContextRecordError::InvalidSessionName => CliError::usage("invalid child context: invalid session name"),
        ChildContextRecordError::InvalidStatus => CliError::usage("invalid child context: invalid status transition"),
        ChildContextRecordError::InvalidText => CliError::usage("invalid child context: contains NUL byte"),
        ChildContextRecordError::InvalidRefs => CliError::usage("invalid child context: refs-jsonl is invalid"),
        ChildContextRecordError::MissingParentSession => CliError::unavailable("cannot record child context: missing parent session"),
        ChildContextRecordError::CannotRecord => CliError::unavailable(format!(
            "cannot record child context: {}",
            ChildContextRecordError::CannotRecord.errno()
        )),
    }
}
