fn parse_valid_agent_schedule_nodes(
    content: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
) -> (Vec<AgentScheduleNode>, Vec<AgentScheduleIssue>) {
    let mut issues = Vec::new();
    if !is_object_name(parent_subject) {
        issues.push(AgentScheduleIssue::InvalidField {
            node: None,
            field: "parent_subject".to_owned(),
            value: parent_subject.to_owned(),
        });
        return (Vec::new(), issues);
    }

    let Ok(value) = serde_json::from_str::<Value>(content) else {
        issues.push(AgentScheduleIssue::InvalidJson);
        return (Vec::new(), issues);
    };
    if !value.is_object() {
        issues.push(AgentScheduleIssue::ScheduleNotObject);
        return (Vec::new(), issues);
    }
    let Ok(schedule) = serde_json::from_value::<ScheduleJson>(value) else {
        issues.push(AgentScheduleIssue::InvalidJson);
        return (Vec::new(), issues);
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
        return (Vec::new(), issues);
    };
    let Some(node_values) = nodes_value.as_array() else {
        issues.push(AgentScheduleIssue::InvalidNodes);
        return (Vec::new(), issues);
    };
    if node_values.is_empty() || node_values.len() > MAX_AGENT_SCHEDULE_NODES {
        issues.push(AgentScheduleIssue::InvalidNodes);
        return (Vec::new(), issues);
    }

    let nodes = inspect_schedule_nodes(node_values, parent_subject, parent_policy, &mut issues);
    (nodes, issues)
}

fn inspect_schedule_nodes(
    node_values: &[Value],
    parent_subject: &str,
    parent_policy: &PolicyV0,
    issues: &mut Vec<AgentScheduleIssue>,
) -> Vec<AgentScheduleNode> {
    let mut nodes = Vec::new();
    let mut seen = ScheduleSeen::default();
    let inspect_context = ScheduleInspectContext {
        parent_subject,
        parent_policy,
    };
    for (index, node_value) in node_values.iter().enumerate() {
        inspect_schedule_node(
            index,
            node_value,
            &inspect_context,
            &mut seen,
            &mut nodes,
            issues,
        );
    }
    inspect_schedule_dependencies(&nodes, issues);
    nodes
}

fn inspect_schedule_node(
    index: usize,
    value: &Value,
    context: &ScheduleInspectContext<'_>,
    seen: &mut ScheduleSeen,
    nodes: &mut Vec<AgentScheduleNode>,
    issues: &mut Vec<AgentScheduleIssue>,
) {
    if !value.is_object() {
        issues.push(AgentScheduleIssue::NodeNotObject { index });
        return;
    }
    let Ok(node) = serde_json::from_value::<ScheduleNodeJson>(value.clone()) else {
        issues.push(AgentScheduleIssue::InvalidJson);
        return;
    };
    let id = required_object_name(None, "id", node.id.as_ref(), issues);
    let Some(id) = id else {
        return;
    };
    if !seen.nodes.insert(id.clone()) {
        issues.push(AgentScheduleIssue::DuplicateNode { node: id.clone() });
    }
    let node_ref = Some(id.clone());

    let kind_text = required_word(node_ref.as_ref(), "kind", node.kind.as_ref(), issues);
    let kind = kind_text.as_deref().and_then(AgentScheduleNodeKind::parse);
    if kind.is_none() {
        issues.push(AgentScheduleIssue::InvalidField {
            node: node_ref.clone(),
            field: "kind".to_owned(),
            value: kind_text.unwrap_or_default(),
        });
    }
    let child = node
        .child
        .as_ref()
        .and_then(|child| required_object_name(node_ref.as_ref(), "child", Some(child), issues));
    let agent = schedule_node_agent(
        node_ref.as_ref(),
        node.agent.as_ref(),
        child.as_ref(),
        issues,
    );
    let child_session = node.session.as_ref().and_then(|session| {
        required_object_name(node_ref.as_ref(), "session", Some(session), issues)
    });
    let handoff = node
        .handoff
        .as_ref()
        .and_then(|handoff| required_handoff_text(node_ref.as_ref(), handoff, issues));
    if child.is_none() && child_session.is_some() {
        issues.push(AgentScheduleIssue::InvalidField {
            node: node_ref.clone(),
            field: "session".to_owned(),
            value: "requires child".to_owned(),
        });
    }
    if child.is_none() && handoff.is_some() {
        issues.push(AgentScheduleIssue::InvalidField {
            node: node_ref.clone(),
            field: "handoff".to_owned(),
            value: "requires child".to_owned(),
        });
    }
    if child.is_some() && handoff.is_none() {
        issues.push(AgentScheduleIssue::MissingHandoff { node: id.clone() });
    }
    if let Some(child) = child.as_ref()
        && !seen.children.insert(child.clone())
    {
        issues.push(AgentScheduleIssue::DuplicateChild {
            child: child.clone(),
        });
    }
    let deps = inspect_string_array(node_ref.as_ref(), "deps", node.deps.as_ref(), issues);
    let max_steps = node.max_steps.as_ref().and_then(Value::as_u64);
    if kind == Some(AgentScheduleNodeKind::React) && !valid_react_bound(node.max_steps.as_ref()) {
        issues.push(AgentScheduleIssue::InvalidReactBound { node: id.clone() });
    }
    if child.is_some()
        && let Some(agent) = agent.as_ref()
        && !context.parent_policy.allows(
            context.parent_subject,
            PolicyObjectClass::Agent,
            agent,
            PolicyPermission::Create,
        )
        && !requires_permission(
            node.requires.as_ref(),
            PolicyObjectClass::Agent,
            agent,
            "create",
        )
    {
        issues.push(AgentScheduleIssue::PermissionNotGranted {
            node: id.clone(),
            class: "agent".to_owned(),
            name: agent.clone(),
            permission: "create".to_owned(),
        });
    }
    inspect_required_permissions(
        &id,
        node.requires.as_ref(),
        context.parent_subject,
        context.parent_policy,
        issues,
    );
    if let (Some(kind), Some(agent)) = (kind, agent) {
        nodes.push(AgentScheduleNode {
            id,
            kind,
            agent,
            child,
            child_session,
            handoff,
            deps,
            max_steps,
        });
    }
}

fn schedule_node_agent(
    node: Option<&String>,
    value: Option<&Value>,
    child: Option<&String>,
    issues: &mut Vec<AgentScheduleIssue>,
) -> Option<String> {
    if let Some(value) = value {
        return required_object_name(node, "agent", Some(value), issues);
    }
    if child.is_some() {
        return Some(DEFAULT_DELEGATED_AGENT.to_owned());
    }
    required_object_name(node, "agent", None, issues)
}
