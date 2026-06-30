/// Inspects a hybrid DAG/ReAct schedule stored as ordinary parent session
/// context, for example `context/plan.json`.
///
/// This is a pure validation helper. It does not create agents, execute tools,
/// watch files, or add a scheduler namespace. The parent agent remains
/// responsible for turning accepted nodes into child-agent handoffs at a Git
/// commit boundary.
#[must_use]
pub fn inspect_agent_schedule_json(
    content: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
) -> AgentScheduleReport {
    let mut issues = Vec::new();
    if !is_object_name(parent_subject) {
        issues.push(AgentScheduleIssue::InvalidField {
            node: None,
            field: "parent_subject".to_owned(),
            value: parent_subject.to_owned(),
        });
        return AgentScheduleReport::new(issues);
    }

    let Ok(value) = serde_json::from_str::<Value>(content) else {
        issues.push(AgentScheduleIssue::InvalidJson);
        return AgentScheduleReport::new(issues);
    };
    if !value.is_object() {
        issues.push(AgentScheduleIssue::ScheduleNotObject);
        return AgentScheduleReport::new(issues);
    }
    let Ok(schedule) = serde_json::from_value::<ScheduleJson>(value) else {
        issues.push(AgentScheduleIssue::InvalidJson);
        return AgentScheduleReport::new(issues);
    };
    if !matches!(schedule.version.as_ref().and_then(Value::as_u64), Some(1)) {
        issues.push(AgentScheduleIssue::InvalidVersion);
    }
    if !matches!(
        schedule.mode.as_ref().and_then(Value::as_str),
        Some("dag-react")
    ) {
        issues.push(AgentScheduleIssue::InvalidMode);
    }

    let Some(nodes_value) = schedule.nodes else {
        issues.push(AgentScheduleIssue::InvalidNodes);
        return AgentScheduleReport::new(issues);
    };
    let Some(node_values) = nodes_value.as_array() else {
        issues.push(AgentScheduleIssue::InvalidNodes);
        return AgentScheduleReport::new(issues);
    };
    if node_values.is_empty() || node_values.len() > MAX_AGENT_SCHEDULE_NODES {
        issues.push(AgentScheduleIssue::InvalidNodes);
        return AgentScheduleReport::new(issues);
    }

    inspect_schedule_nodes(node_values, parent_subject, parent_policy, &mut issues);

    AgentScheduleReport::new(issues)
}

/// Returns the currently ready nodes from a valid hybrid DAG/ReAct schedule.
///
/// `completed_nodes` are node ids whose durable results are already available
/// to the parent. The returned nodes are not completed and have all deps
/// completed. Order follows the schedule file, so callers can choose how much
/// parallelism to use without another scheduler namespace.
pub fn ready_agent_schedule_nodes(
    content: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
    completed_nodes: &[&str],
) -> Result<Vec<AgentScheduleNode>, AgentScheduleReport> {
    let (nodes, mut issues) =
        parse_valid_agent_schedule_nodes(content, parent_subject, parent_policy);
    if issues.is_empty() {
        inspect_completed_nodes(&nodes, completed_nodes, &mut issues);
    }
    if !issues.is_empty() {
        return Err(AgentScheduleReport::new(issues));
    }
    let completed = completed_nodes.iter().copied().collect::<HashSet<_>>();
    Ok(nodes
        .into_iter()
        .filter(|node| {
            !completed.contains(node.id())
                && node
                    .deps()
                    .iter()
                    .all(|dep| completed.contains(dep.as_str()))
        })
        .collect())
}

/// Returns validated schedule nodes from a hybrid DAG/ReAct schedule.
pub fn agent_schedule_nodes(
    content: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
) -> Result<Vec<AgentScheduleNode>, AgentScheduleReport> {
    let (nodes, issues) = parse_valid_agent_schedule_nodes(content, parent_subject, parent_policy);
    if issues.is_empty() {
        Ok(nodes)
    } else {
        Err(AgentScheduleReport::new(issues))
    }
}

/// Returns ready delegated child handoffs from a valid hybrid schedule.
///
/// This derives parent-owned `context/child/<child>/handoff.md` inputs. It does
/// not create agents, start runtimes, or mark nodes complete.
pub fn ready_agent_schedule_child_handoffs(
    content: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
    completed_nodes: &[&str],
    default_child_session: &str,
) -> Result<Vec<AgentScheduleChildHandoff>, AgentScheduleReport> {
    if !is_object_name(default_child_session) {
        return Err(AgentScheduleReport::new(vec![
            AgentScheduleIssue::InvalidField {
                node: None,
                field: "default_child_session".to_owned(),
                value: default_child_session.to_owned(),
            },
        ]));
    }
    Ok(
        ready_agent_schedule_nodes(content, parent_subject, parent_policy, completed_nodes)?
            .into_iter()
            .filter_map(|node| {
                Some(AgentScheduleChildHandoff {
                    node: node.id,
                    child: node.child?,
                    agent: node.agent,
                    session: node
                        .child_session
                        .unwrap_or_else(|| default_child_session.to_owned()),
                    handoff: node.handoff?,
                })
            })
            .collect(),
    )
}
