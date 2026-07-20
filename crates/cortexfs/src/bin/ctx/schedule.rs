use crate::*;

pub(crate) fn schedule_command(root: &Path, args: &ScheduleArgs) -> Result<(), CliError> {
    match *args {
        ScheduleArgs::Status { ref path, ref done } => schedule_status(root, path, done),
        ScheduleArgs::Advance { ref path, ref done } => schedule_advance(root, path, done),
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
        } => schedule_result(
            root,
            path,
            child,
            ScheduleResultInput {
                status,
                result,
                refs_jsonl,
            },
        ),
    }
}

pub(crate) fn schedule_status(root: &Path, path: &str, done: &[String]) -> Result<(), CliError> {
    let schedule = load_schedule_context(root, path, "status")?;
    for line in schedule_status_lines(root, &schedule, done)? {
        print_line(&line)?;
    }
    Ok(())
}

pub(crate) fn schedule_status_lines(
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
    .map_err(|report| {
        schedule_record_cli_error("status", AgentScheduleRecordError::InvalidSchedule(report))
    })?;
    let ready = ready
        .iter()
        .map(AgentScheduleNode::id)
        .collect::<HashSet<_>>();
    let completed = completed.iter().map(String::as_str).collect::<HashSet<_>>();
    let mut lines = Vec::new();
    for node in agent_schedule_nodes(
        &schedule.json,
        &schedule.parent_subject,
        &schedule.parent_policy,
    )
    .map_err(|report| {
        schedule_record_cli_error("status", AgentScheduleRecordError::InvalidSchedule(report))
    })? {
        let (model, life, child_parent) = schedule_node_agent_details(root, &node)?;
        lines.push(format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            terminal_safe_text(node.id()),
            node.kind().as_str(),
            terminal_safe_text(node.agent()),
            terminal_safe_text(node.child().unwrap_or("-")),
            terminal_safe_text(&schedule_node_session(&schedule.parent_session_dir, &node)?),
            terminal_safe_text(&model),
            terminal_safe_text(&life),
            node.child()
                .map_or("-", |_| agent_role_for_display(node.agent())),
            terminal_safe_text(&child_parent),
            schedule_node_state(root, schedule, &completed, &ready, &node)?,
        ));
    }
    Ok(lines)
}

pub(crate) fn schedule_node_agent_details(
    root: &Path,
    node: &AgentScheduleNode,
) -> Result<(String, String, String), CliError> {
    if node.child().is_some() {
        schedule_handoff_agent_details(root, node.agent())
    } else {
        Ok(("-".to_owned(), "-".to_owned(), "-".to_owned()))
    }
}

pub(crate) fn schedule_node_session(
    parent_session_dir: &Path,
    node: &AgentScheduleNode,
) -> Result<String, CliError> {
    if node.child().is_none() {
        return Ok("-".to_owned());
    }
    Ok(node.child_session().map_or(
        schedule_parent_session_for_output(parent_session_dir)?,
        str::to_owned,
    ))
}

pub(crate) struct LoadedScheduleContext {
    pub(crate) abi_path: String,
    pub(crate) context_abi_path: String,
    pub(crate) parent_agent: String,
    pub(crate) parent_session_dir: PathBuf,
    pub(crate) parent_subject: String,
    pub(crate) parent_policy: PolicyV0,
    pub(crate) json: String,
    plan_path: PathBuf,
    plan_file: fs::File,
    plan_dev: u64,
    plan_ino: u64,
}

struct OpenedSchedulePlan {
    file: fs::File,
    dev: u64,
    ino: u64,
    json: String,
}

fn open_schedule_plan(path: &Path) -> Result<OpenedSchedulePlan, CliError> {
    let mut file = open_plain_read_file(path)?;
    let metadata = file.metadata().map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
    })?;
    if !metadata.is_file() || metadata.len() > MAX_CTX_FILE_CHECK_BYTES {
        return Err(CliError::unavailable(format!(
            "cannot read {}: not a small regular file",
            path.display()
        )));
    }
    let len = usize::try_from(metadata.len()).map_err(|error| {
        CliError::unavailable(format!("cannot read {}: {error}", path.display()))
    })?;
    let mut content = vec![0; len];
    file.read_exact(&mut content).map_err(|error| {
        CliError::unavailable(format!("cannot read {}: {error}", path.display()))
    })?;
    let after = file.metadata().map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
    })?;
    let current = open_plain_read_file(path)?;
    let current_metadata = current.metadata().map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
    })?;
    if (
        after.dev(),
        after.ino(),
        after.len(),
        after.mtime(),
        after.mtime_nsec(),
    ) != (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
    ) || (
        current_metadata.dev(),
        current_metadata.ino(),
        current_metadata.len(),
        current_metadata.mtime(),
        current_metadata.mtime_nsec(),
    ) != (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
    ) {
        return Err(CliError::usage(
            "invalid child context: current plan changed during read",
        ));
    }
    let json = String::from_utf8(content).map_err(|error| {
        CliError::unavailable(format!("cannot read {}: {error}", path.display()))
    })?;
    Ok(OpenedSchedulePlan {
        file: current,
        dev: metadata.dev(),
        ino: metadata.ino(),
        json,
    })
}

pub(crate) fn load_schedule_context(
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
    let parent_agent = parent_agent_for_session_context_path(&abi_path).ok_or_else(|| {
        CliError::usage(format!(
            "schedule {command} plan must belong to an agent session"
        ))
    })?;
    let plan_path = resolve_abi_path(root, path)?;
    let plan = open_schedule_plan(&plan_path)?;
    Ok(LoadedScheduleContext {
        context_abi_path: schedule_context_abi_path(&abi_path, command)?,
        parent_agent: parent_agent.to_owned(),
        parent_session_dir: schedule_parent_session_dir(root, path, command)?,
        parent_subject: parent_policy_subject(root, parent_agent)?,
        parent_policy: parent_agent_policy(root, parent_agent)?,
        json: plan.json,
        plan_path,
        plan_file: plan.file,
        plan_dev: plan.dev,
        plan_ino: plan.ino,
        abi_path,
    })
}

pub(crate) fn schedule_node_state(
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
        let child_dir = child_context_dir(root, &child_paths.status, child)?;
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

pub(crate) fn schedule_parent_session_dir(
    root: &Path,
    path: &str,
    command: &str,
) -> Result<PathBuf, CliError> {
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
        .ok_or_else(|| {
            CliError::usage(format!("schedule {command} requires an agent session plan"))
        })
}

pub(crate) fn schedule_advance(root: &Path, path: &str, done: &[String]) -> Result<(), CliError> {
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
        let (model, life, child_parent) = schedule_handoff_agent_details(root, handoff.agent())?;
        print_line(&format!(
            "handoff node={} child={} agent={} session={} model={} life={} role={} parent={} child_parent={} plan={} handoff={} result={} refs={}",
            handoff.node(),
            handoff.child(),
            handoff.agent(),
            handoff.session(),
            model,
            life,
            agent_role_for_display(handoff.agent()),
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

pub(crate) fn schedule_handoff_agent_details(
    root: &Path,
    agent: &str,
) -> Result<(String, String, String), CliError> {
    require_schedule_handoff_agent(root, agent)?;
    let control = agent_control_dir(root, agent);
    let (model, life) = read_agent_model_life_for_context(&control, "handoff agent")?;
    let parent = read_agent_parent_ref(&control)?
        .as_ref()
        .map_or_else(|| "-".to_owned(), agent_parent_ref_display);
    Ok((model, life, parent))
}

pub(crate) fn schedule_require_handoff_parent(
    parent_ref: &str,
    agent: &str,
    child_parent: &str,
) -> Result<(), CliError> {
    let parent = parse_agent_parent_ref(parent_ref)
        .ok_or_else(|| CliError::unavailable("cannot derive schedule parent"))?;
    let child = parse_agent_parent_ref(child_parent).ok_or_else(|| {
        CliError::usage(format!(
            "invalid handoff agent parent for {agent}: {child_parent}"
        ))
    })?;
    if !agent_parent_ref_matches(
        &child,
        &parent.agent,
        parent.session.as_deref(),
        parent.run.as_deref(),
    ) {
        return Err(CliError::usage(format!(
            "handoff agent parent mismatch for {agent}: {child_parent}"
        )));
    }
    Ok(())
}

pub(crate) fn agent_role_for_display(agent: &str) -> &'static str {
    if is_worker_agent_name(agent) {
        "worker"
    } else {
        "agent"
    }
}

pub(crate) fn require_schedule_handoff_agent(root: &Path, agent: &str) -> Result<(), CliError> {
    let object = agent_object_path(root, agent);
    open_executable_no_follow(&object)
        .map(drop)
        .map_err(|error| {
            CliError::usage(format!(
                "missing handoff agent object {agent}: {}",
                error.message
            ))
        })?;
    let control = agent_control_dir(root, agent);
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

pub(crate) struct ScheduleChildHandoffContext {
    pub(crate) schedule: LoadedScheduleContext,
    pub(crate) child_dir: PathBuf,
    pub(crate) child_paths: ScheduleChildContextAbiPaths,
    pub(crate) agent: String,
    pub(crate) session: String,
    pub(crate) handoff: String,
    pub(crate) model: String,
    pub(crate) life: String,
    pub(crate) parent_ref: String,
    pub(crate) child_parent: String,
}

pub(crate) fn schedule_child_handoff_context(
    root: &Path,
    path: &str,
    child: &str,
    command: &str,
) -> Result<ScheduleChildHandoffContext, CliError> {
    let schedule = load_schedule_context(root, path, command)?;
    let node = schedule_delegated_child_node(&schedule, child, command)?;
    let child_paths = schedule_child_context_abi_paths(&schedule.context_abi_path, child)?;
    let child_dir = child_context_dir(root, &child_paths.status, child)?;
    let (agent, session) = schedule_child_context_agent_session(&child_dir)?;
    let expected_session = schedule_node_session(&schedule.parent_session_dir, &node)?;
    let expected_handoff =
        ensure_trailing_newline(node.handoff().ok_or_else(|| {
            CliError::usage("invalid child context: current plan handoff mismatch")
        })?);
    let handoff = read_file_to_string(&child_dir.join("handoff.md"))?;
    if agent != node.agent() || session != expected_session || handoff != expected_handoff {
        return Err(CliError::usage(
            "invalid child context: current plan handoff mismatch",
        ));
    }
    let (model, life, child_parent) = schedule_handoff_agent_details(root, node.agent())?;
    let parent_ref =
        schedule_parent_ref_for_output(&schedule.parent_agent, &schedule.parent_session_dir)?;
    schedule_require_handoff_parent(&parent_ref, &agent, &child_parent)?;
    Ok(ScheduleChildHandoffContext {
        schedule,
        child_dir,
        child_paths,
        agent,
        session,
        handoff,
        model,
        life,
        parent_ref,
        child_parent,
    })
}

pub(crate) fn schedule_delegated_child_node(
    schedule: &LoadedScheduleContext,
    child: &str,
    command: &str,
) -> Result<AgentScheduleNode, CliError> {
    schedule_delegated_child_node_from(
        &schedule.json,
        &schedule.parent_subject,
        &schedule.parent_policy,
        child,
        command,
    )
}

fn schedule_delegated_child_node_from(
    json: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
    child: &str,
    command: &str,
) -> Result<AgentScheduleNode, CliError> {
    let nodes = agent_schedule_nodes(json, parent_subject, parent_policy).map_err(|report| {
        schedule_record_cli_error(command, AgentScheduleRecordError::InvalidSchedule(report))
    })?;
    let mut matches = nodes.into_iter().filter(|node| node.child() == Some(child));
    let node = matches.next().ok_or_else(|| {
        CliError::usage("invalid child context: child is not delegated by current plan")
    })?;
    if matches.next().is_some() {
        return Err(CliError::usage(
            "invalid child context: child is delegated more than once by current plan",
        ));
    }
    Ok(node)
}

fn revalidate_schedule_child(
    context: &ScheduleChildHandoffContext,
    child: &str,
    command: &str,
) -> Result<OpenedSchedulePlan, CliError> {
    let schedule = &context.schedule;
    let held = schedule.plan_file.metadata().map_err(|error| {
        CliError::unavailable(format!(
            "cannot {command} child: cannot stat held plan: {error}"
        ))
    })?;
    let current = open_schedule_plan(&schedule.plan_path)?;
    if !held.is_file()
        || (held.dev(), held.ino()) != (schedule.plan_dev, schedule.plan_ino)
        || (current.dev, current.ino) != (schedule.plan_dev, schedule.plan_ino)
        || current.json != schedule.json
    {
        return Err(CliError::usage(
            "invalid child context: current plan changed during operation",
        ));
    }
    let node = schedule_delegated_child_node_from(
        &current.json,
        &schedule.parent_subject,
        &schedule.parent_policy,
        child,
        command,
    )?;
    let expected_session = schedule_node_session(&schedule.parent_session_dir, &node)?;
    let expected_handoff =
        ensure_trailing_newline(node.handoff().ok_or_else(|| {
            CliError::usage("invalid child context: current plan handoff mismatch")
        })?);
    if node.agent() != context.agent
        || expected_session != context.session
        || expected_handoff != context.handoff
    {
        return Err(CliError::usage(
            "invalid child context: current plan handoff mismatch",
        ));
    }
    Ok(current)
}

fn prepare_schedule_child_mutation(
    root: &Path,
    path: &str,
    child: &str,
    command: &str,
    hook: impl FnOnce() -> Result<(), CliError>,
) -> Result<
    (
        ScheduleChildHandoffContext,
        ChildHandoffReceipt,
        ChildContextLease,
        OpenedSchedulePlan,
    ),
    CliError,
> {
    let context = schedule_child_handoff_context(root, path, child, command)?;
    let receipt =
        child_handoff_receipt(&context.child_dir).map_err(schedule_child_context_cli_error)?;
    let lease = acquire_child_context_lease(&receipt).map_err(schedule_child_context_cli_error)?;
    hook()?;
    validate_child_context_lease(
        &lease,
        &receipt,
        &context.agent,
        &context.session,
        &context.handoff,
    )
    .map_err(schedule_child_context_cli_error)?;
    let plan = revalidate_schedule_child(&context, child, command)?;
    Ok((context, receipt, lease, plan))
}

pub(crate) fn schedule_claim(root: &Path, path: &str, child: &str) -> Result<(), CliError> {
    schedule_claim_with_hook(root, path, child, || Ok(()))
}

pub(crate) fn schedule_claim_with_hook(
    root: &Path,
    path: &str,
    child: &str,
    hook: impl FnOnce() -> Result<(), CliError>,
) -> Result<(), CliError> {
    let (handoff, receipt, lease, _plan) =
        prepare_schedule_child_mutation(root, path, child, "claim", hook)?;
    if child_context_lease_status(&lease).map_err(schedule_child_context_cli_error)?
        != ChildContextStatus::Active
    {
        claim_child_handoff_active_with_lease(
            &lease,
            &receipt,
            &handoff.agent,
            &handoff.session,
            Some(&handoff.handoff),
        )
        .map_err(schedule_child_context_cli_error)?;
    }
    print_line(&format!(
        "claim child={child} status=active {} handoff={} result={} refs={}",
        schedule_handoff_identity_output(&handoff),
        handoff.child_paths.handoff,
        handoff.child_paths.result,
        handoff.child_paths.refs,
    ))
}

pub(crate) fn schedule_handoff_identity_output(handoff: &ScheduleChildHandoffContext) -> String {
    format!(
        "agent={} session={} model={} life={} role={} parent={} child_parent={} plan={}",
        terminal_safe_text(&handoff.agent),
        terminal_safe_text(&handoff.session),
        terminal_safe_text(&handoff.model),
        terminal_safe_text(&handoff.life),
        agent_role_for_display(&handoff.agent),
        shell_quote_arg(&handoff.parent_ref),
        shell_quote_arg(&handoff.child_parent),
        handoff.schedule.abi_path,
    )
}

pub(crate) fn child_context_dir(
    root: &Path,
    status_abi_path: &str,
    child: &str,
) -> Result<PathBuf, CliError> {
    resolve_abi_path(root, status_abi_path)?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| CliError::usage(format!("invalid child status path for {child}")))
}

pub(crate) fn schedule_child_context_agent_session(
    child_dir: &Path,
) -> Result<(String, String), CliError> {
    let agent = read_optional_trimmed(&child_dir.join("agent"))?
        .ok_or_else(|| CliError::usage("invalid child context: invalid agent name"))?;
    let session = read_optional_trimmed(&child_dir.join("session"))?
        .ok_or_else(|| CliError::usage("invalid child context: invalid session name"))?;
    if !is_object_name(&agent) {
        return Err(CliError::usage("invalid child context: invalid agent name"));
    }
    if !is_object_name(&session) {
        return Err(CliError::usage(
            "invalid child context: invalid session name",
        ));
    }
    Ok((agent, session))
}

pub(crate) fn schedule_parent_ref_for_output(
    parent_agent: &str,
    parent_session_dir: &Path,
) -> Result<String, CliError> {
    let session = schedule_parent_session_for_output(parent_session_dir)?;
    Ok(format!("agent:{parent_agent} session:{session}"))
}

pub(crate) fn schedule_parent_session_for_output(
    parent_session_dir: &Path,
) -> Result<String, CliError> {
    let session = parent_session_dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| is_object_name(name))
        .ok_or_else(|| CliError::unavailable("cannot derive parent session for schedule"))?;
    Ok(session.to_owned())
}

#[derive(Clone, Copy)]
pub(crate) struct ScheduleResultInput<'a> {
    pub(crate) status: ChildContextStatus,
    pub(crate) result: &'a str,
    pub(crate) refs_jsonl: &'a str,
}

pub(crate) fn schedule_result(
    root: &Path,
    path: &str,
    child: &str,
    input: ScheduleResultInput<'_>,
) -> Result<(), CliError> {
    schedule_result_with_hook(root, path, child, input, || Ok(()))
}

pub(crate) fn schedule_result_with_hook(
    root: &Path,
    path: &str,
    child: &str,
    input: ScheduleResultInput<'_>,
    hook: impl FnOnce() -> Result<(), CliError>,
) -> Result<(), CliError> {
    let (handoff, receipt, lease, _plan) =
        prepare_schedule_child_mutation(root, path, child, "result", hook)?;
    finish_child_result_with_lease(
        lease,
        &receipt,
        &handoff.agent,
        &handoff.session,
        Some(&handoff.handoff),
        input.status,
        input.result,
        input.refs_jsonl,
    )
    .map_err(schedule_child_context_cli_error)?;
    print_line(&format!(
        "result child={child} status={} {} result={} refs={}",
        input.status.as_str(),
        schedule_handoff_identity_output(&handoff),
        handoff.child_paths.result,
        handoff.child_paths.refs,
    ))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ScheduleChildContextAbiPaths {
    pub(crate) status: String,
    pub(crate) handoff: String,
    pub(crate) result: String,
    pub(crate) refs: String,
}

pub(crate) fn schedule_context_abi_path(abi_path: &str, command: &str) -> Result<String, CliError> {
    abi_path
        .strip_suffix("/plan.json")
        .map(str::to_owned)
        .ok_or_else(|| CliError::usage(format!("schedule {command} requires context/plan.json")))
}

pub(crate) fn schedule_child_context_abi_paths(
    context_abi_path: &str,
    child: &str,
) -> Result<ScheduleChildContextAbiPaths, CliError> {
    if !is_object_name(child) {
        return Err(CliError::usage(
            "invalid child context path: invalid child name",
        ));
    }
    let base = format!("{context_abi_path}/child/{child}");
    Ok(ScheduleChildContextAbiPaths {
        status: format!("{base}/status"),
        handoff: format!("{base}/handoff.md"),
        result: format!("{base}/result.md"),
        refs: format!("{base}/refs.jsonl"),
    })
}

pub(crate) fn schedule_record_cli_error(
    command: &str,
    error: AgentScheduleRecordError,
) -> CliError {
    match error {
        AgentScheduleRecordError::InvalidText => {
            CliError::usage("invalid agent schedule: contains NUL byte")
        }
        AgentScheduleRecordError::InvalidSchedule(report) => CliError::usage(format!(
            "invalid agent schedule: {}",
            format_agent_schedule_issues(report.issues())
        )),
        AgentScheduleRecordError::MissingParentSession => CliError::unavailable(format!(
            "cannot {command} agent schedule: missing parent session"
        )),
        AgentScheduleRecordError::CannotRecord => CliError::unavailable(format!(
            "cannot {command} agent schedule: {}",
            AgentScheduleRecordError::CannotRecord.errno()
        )),
    }
}

pub(crate) fn schedule_child_context_cli_error(error: ChildContextRecordError) -> CliError {
    match error {
        ChildContextRecordError::InvalidChildName => {
            CliError::usage("invalid child context: invalid child name")
        }
        ChildContextRecordError::InvalidAgentName => {
            CliError::usage("invalid child context: invalid agent name")
        }
        ChildContextRecordError::InvalidSessionName => {
            CliError::usage("invalid child context: invalid session name")
        }
        ChildContextRecordError::InvalidStatus => {
            CliError::usage("invalid child context: invalid status transition")
        }
        ChildContextRecordError::InvalidText => {
            CliError::usage("invalid child context: contains NUL byte")
        }
        ChildContextRecordError::InvalidRefs => {
            CliError::usage("invalid child context: refs-jsonl is invalid")
        }
        ChildContextRecordError::MissingParentSession => {
            CliError::unavailable("cannot record child context: missing parent session")
        }
        ChildContextRecordError::CannotRecord => CliError::unavailable(format!(
            "cannot record child context: {}",
            ChildContextRecordError::CannotRecord.errno()
        )),
    }
}
