#[test]
fn agent_schedule_accepts_bounded_dag_react_plan_with_parent_permissions() {
    let policy = ok!(PolicyV0::parse(
        "\
allow planner_t tool:fs.read execute
allow planner_t model:openai/gpt-5.5 use
allow planner_t agent:reviewer create
allow planner_t session:default write
"
    ));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "plan",
      "kind": "dag",
      "agent": "planner",
      "requires": [
        {"class": "tool", "name": "fs.read", "permission": "execute"},
        {"class": "model", "name": "openai/gpt-5.5", "permission": "use"}
      ]
    },
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "rev-123",
      "handoff": "Task: review the plan\n",
      "deps": ["plan"],
      "max_steps": 8,
      "requires": [
        {"class": "agent", "name": "reviewer", "permission": "create"},
        {"class": "session", "name": "default", "permission": "write"}
      ]
    }
  ]
}
"#;

    let report = inspect_agent_schedule_json(schedule, "planner_t", &policy);
    assert_eq!(report.issues(), &[]);
}

#[test]
fn agent_schedule_rejects_cycles_unknown_deps_unbounded_react_and_permission_expansion() {
    let policy = ok!(PolicyV0::parse("allow planner_t tool:fs.read execute\n"));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "plan",
      "kind": "dag",
      "agent": "planner",
      "deps": ["review"]
    },
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "deps": ["plan", "missing"],
      "max_steps": 0,
      "requires": [
        {"class": "tool", "name": "shell.exec", "permission": "execute"}
      ]
    }
  ]
}
"#;

    let report = inspect_agent_schedule_json(schedule, "planner_t", &policy);
    assert!(report.issues().contains(&AgentScheduleIssue::UnknownDependency {
        node: "review".to_owned(),
        dependency: "missing".to_owned()
    }));
    assert!(report.issues().contains(&AgentScheduleIssue::DependencyCycle {
        node: "plan".to_owned()
    }));
    assert!(report.issues().contains(&AgentScheduleIssue::InvalidReactBound {
        node: "review".to_owned()
    }));
    assert!(report.issues().contains(&AgentScheduleIssue::PermissionNotGranted {
        node: "review".to_owned(),
        class: "tool".to_owned(),
        name: "shell.exec".to_owned(),
        permission: "execute".to_owned()
    }));
}

#[test]
fn agent_schedule_rejects_invalid_shape_and_duplicate_nodes() {
    let policy = ok!(PolicyV0::parse(""));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {"id": "same", "kind": "dag", "agent": "planner"},
    {"id": "same", "kind": "dag", "agent": "planner"}
  ]
}
"#;

    let report = inspect_agent_schedule_json(schedule, "planner_t", &policy);
    assert_eq!(
        report.issues(),
        &[AgentScheduleIssue::DuplicateNode {
            node: "same".to_owned()
        }]
    );

    let report = inspect_agent_schedule_json("[]", "planner_t", &policy);
    assert_eq!(report.issues(), &[AgentScheduleIssue::ScheduleNotObject]);
}

#[test]
fn agent_schedule_ready_nodes_follow_dag_dependencies_in_plan_order() {
    let policy = ok!(PolicyV0::parse(
        "\
allow planner_t tool:fs.read execute
allow planner_t agent:reviewer create
allow planner_t agent:executor create
"
    ));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "plan",
      "kind": "dag",
      "agent": "planner",
      "requires": [
        {"class": "tool", "name": "fs.read", "permission": "execute"}
      ]
    },
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "rev-123",
      "handoff": "Task: review the plan\n",
      "deps": ["plan"],
      "max_steps": 8,
      "requires": [
        {"class": "agent", "name": "reviewer", "permission": "create"}
      ]
    },
    {
      "id": "execute",
      "kind": "dag",
      "agent": "executor",
      "child": "exec-123",
      "session": "run-123",
      "handoff": "Task: execute the accepted plan\n",
      "deps": ["plan"],
      "requires": [
        {"class": "agent", "name": "executor", "permission": "create"}
      ]
    }
  ]
}
"#;

    let ready = ok!(ready_agent_schedule_nodes(
        schedule,
        "planner_t",
        &policy,
        &[]
    ));
    assert_eq!(ready.len(), 1);
    let Some(first) = ready.first() else {
        return;
    };
    assert_eq!(first.id(), "plan");
    assert_eq!(first.kind(), AgentScheduleNodeKind::Dag);
    assert_eq!(first.agent(), "planner");
    assert_eq!(first.child(), None);
    assert!(first.deps().is_empty());

    let ready = ok!(ready_agent_schedule_nodes(
        schedule,
        "planner_t",
        &policy,
        &["plan"]
    ));
    assert_eq!(ready.len(), 2);
    let Some(review) = ready.first() else {
        return;
    };
    let Some(execute) = ready.get(1) else {
        return;
    };
    assert_eq!(review.id(), "review");
    assert_eq!(execute.id(), "execute");
    assert_eq!(review.kind(), AgentScheduleNodeKind::React);
    assert_eq!(review.child(), Some("rev-123"));
    assert_eq!(review.child_session(), Some("default"));
    assert_eq!(review.handoff(), Some("Task: review the plan\n"));
    assert_eq!(review.max_steps(), Some(8));
    assert_eq!(execute.child(), Some("exec-123"));
    assert_eq!(execute.child_session(), Some("run-123"));

    let ready = ok!(ready_agent_schedule_nodes(
        schedule,
        "planner_t",
        &policy,
        &["plan", "review", "execute"]
    ));
    assert_eq!(ready, []);
}

#[test]
fn agent_schedule_ready_nodes_reject_unknown_completed_nodes_and_invalid_plan() {
    let policy = ok!(PolicyV0::parse("allow planner_t tool:fs.read execute\n"));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "plan",
      "kind": "dag",
      "agent": "planner",
      "requires": [
        {"class": "tool", "name": "fs.read", "permission": "execute"}
      ]
    }
  ]
}
"#;

    let result = ready_agent_schedule_nodes(
        schedule,
        "planner_t",
        &policy,
        &["missing"]
    );
    assert!(result.is_err());
    let Err(report) = result else {
        return;
    };
    assert_eq!(
        report.issues(),
        &[AgentScheduleIssue::UnknownCompletedNode {
            node: "missing".to_owned()
        }]
    );

    let result = ready_agent_schedule_nodes("[]", "planner_t", &policy, &[]);
    assert!(result.is_err());
    let Err(report) = result else {
        return;
    };
    assert_eq!(report.issues(), &[AgentScheduleIssue::ScheduleNotObject]);
}

#[test]
fn agent_schedule_child_handoffs_include_only_ready_delegated_nodes() {
    let policy = ok!(PolicyV0::parse(
        "\
allow planner_t tool:fs.read execute
allow planner_t agent:reviewer create
allow planner_t agent:executor create
"
    ));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "plan",
      "kind": "dag",
      "agent": "planner",
      "requires": [
        {"class": "tool", "name": "fs.read", "permission": "execute"}
      ]
    },
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "rev-123",
      "handoff": "Task: review the plan\n",
      "deps": ["plan"],
      "max_steps": 8,
      "requires": [
        {"class": "agent", "name": "reviewer", "permission": "create"}
      ]
    },
    {
      "id": "execute",
      "kind": "dag",
      "agent": "executor",
      "child": "exec-123",
      "session": "run-123",
      "handoff": "Task: execute the accepted plan\n",
      "deps": ["review"],
      "requires": [
        {"class": "agent", "name": "executor", "permission": "create"}
      ]
    }
  ]
}
"#;

    let handoffs = ok!(ready_agent_schedule_child_handoffs(
        schedule,
        "planner_t",
        &policy,
        &[]
    ));
    assert_eq!(handoffs, []);

    let handoffs = ok!(ready_agent_schedule_child_handoffs(
        schedule,
        "planner_t",
        &policy,
        &["plan"]
    ));
    assert_eq!(handoffs.len(), 1);
    let Some(handoff) = handoffs.first() else {
        return;
    };
    assert_eq!(handoff.node(), "review");
    assert_eq!(handoff.child(), "rev-123");
    assert_eq!(handoff.agent(), "reviewer");
    assert_eq!(handoff.session(), "default");
    assert_eq!(handoff.handoff(), "Task: review the plan\n");

    let handoffs = ok!(ready_agent_schedule_child_handoffs(
        schedule,
        "planner_t",
        &policy,
        &["plan", "review"]
    ));
    assert_eq!(handoffs.len(), 1);
    let Some(handoff) = handoffs.first() else {
        return;
    };
    assert_eq!(handoff.node(), "execute");
    assert_eq!(handoff.session(), "run-123");
}

#[test]
fn agent_schedule_rejects_delegated_node_without_handoff() {
    let policy = ok!(PolicyV0::parse("allow planner_t agent:reviewer create\n"));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "rev-123",
      "max_steps": 8,
      "requires": [
        {"class": "agent", "name": "reviewer", "permission": "create"}
      ]
    }
  ]
}
"#;

    let report = inspect_agent_schedule_json(schedule, "planner_t", &policy);
    assert_eq!(
        report.issues(),
        &[AgentScheduleIssue::MissingHandoff {
            node: "review".to_owned()
        }]
    );
}

#[test]
fn agent_schedule_rejects_child_only_fields_without_child_channel() {
    let policy = ok!(PolicyV0::parse(""));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "session": "default",
      "handoff": "Task: review the plan\n",
      "max_steps": 8
    }
  ]
}
"#;

    let report = inspect_agent_schedule_json(schedule, "planner_t", &policy);
    assert_eq!(
        report.issues(),
        &[
            AgentScheduleIssue::InvalidField {
                node: Some("review".to_owned()),
                field: "session".to_owned(),
                value: "requires child".to_owned()
            },
            AgentScheduleIssue::InvalidField {
                node: Some("review".to_owned()),
                field: "handoff".to_owned(),
                value: "requires child".to_owned()
            }
        ]
    );
}

#[test]
fn agent_schedule_rejects_delegated_node_without_parent_create_authority() {
    let policy = ok!(PolicyV0::parse(""));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "rev-123",
      "handoff": "Task: review the plan\n",
      "max_steps": 8
    }
  ]
}
"#;

    let report = inspect_agent_schedule_json(schedule, "planner_t", &policy);
    assert_eq!(
        report.issues(),
        &[AgentScheduleIssue::PermissionNotGranted {
            node: "review".to_owned(),
            class: "agent".to_owned(),
            name: "reviewer".to_owned(),
            permission: "create".to_owned()
        }]
    );
}

#[test]
fn agent_schedule_rejects_duplicate_child_result_channels() {
    let policy = ok!(PolicyV0::parse(
        "\
allow planner_t agent:reviewer create
allow planner_t agent:executor create
"
    ));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "shared-child",
      "handoff": "Task: review the plan\n",
      "max_steps": 8,
      "requires": [
        {"class": "agent", "name": "reviewer", "permission": "create"}
      ]
    },
    {
      "id": "execute",
      "kind": "dag",
      "agent": "executor",
      "child": "shared-child",
      "handoff": "Task: execute the plan\n",
      "requires": [
        {"class": "agent", "name": "executor", "permission": "create"}
      ]
    }
  ]
}
"#;

    let report = inspect_agent_schedule_json(schedule, "planner_t", &policy);
    assert!(report.issues().contains(&AgentScheduleIssue::DuplicateChild {
        child: "shared-child".to_owned()
    }));
}

#[test]
fn agent_schedule_recorder_materializes_ready_child_handoffs() {
    let root = clean_test_dir("agent-schedule-ready-handoff-record");
    let session = root.join("default");
    create_complete_session_layout(&session);
    let policy = ok!(PolicyV0::parse(
        "\
allow planner_t tool:fs.read execute
allow planner_t agent:reviewer create
allow planner_t agent:executor create
"
    ));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "plan",
      "kind": "dag",
      "agent": "planner",
      "requires": [
        {"class": "tool", "name": "fs.read", "permission": "execute"}
      ]
    },
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "rev-123",
      "handoff": "Task: review the plan\n",
      "deps": ["plan"],
      "max_steps": 8,
      "requires": [
        {"class": "agent", "name": "reviewer", "permission": "create"}
      ]
    },
    {
      "id": "execute",
      "kind": "dag",
      "agent": "executor",
      "child": "exec-123",
      "handoff": "Task: execute the accepted plan\n",
      "deps": ["review"],
      "requires": [
        {"class": "agent", "name": "executor", "permission": "create"}
      ]
    }
  ]
}
"#;

    let handoffs = ok!(record_ready_agent_schedule_child_handoffs_to_parent_context(
        &session,
        schedule,
        "planner_t",
        &policy,
        &["plan"]
    ));

    assert_eq!(handoffs.len(), 1);
    assert_file_text(
        &session
            .join("context")
            .join("child")
            .join("rev-123")
            .join("agent"),
        "reviewer\n",
    );
    assert_file_text(
        &session
            .join("context")
            .join("child")
            .join("rev-123")
            .join("session"),
        "default\n",
    );
    assert_file_text(
        &session
            .join("context")
            .join("child")
            .join("rev-123")
            .join("handoff.md"),
        "Task: review the plan\n",
    );
    assert!(!session
        .join("context")
        .join("child")
        .join("exec-123")
        .exists());
}

#[test]
fn agent_schedule_completion_derives_done_delegated_nodes_from_child_status() {
    let root = clean_test_dir("agent-schedule-completed-from-child");
    let session = root.join("default");
    create_complete_session_layout(&session);
    let policy = ok!(PolicyV0::parse(
        "\
allow planner_t tool:fs.read execute
allow planner_t agent:reviewer create
allow planner_t agent:executor create
"
    ));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "plan",
      "kind": "dag",
      "agent": "planner",
      "requires": [
        {"class": "tool", "name": "fs.read", "permission": "execute"}
      ]
    },
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "rev-123",
      "handoff": "Task: review the plan\n",
      "deps": ["plan"],
      "max_steps": 8,
      "requires": [
        {"class": "agent", "name": "reviewer", "permission": "create"}
      ]
    },
    {
      "id": "execute",
      "kind": "dag",
      "agent": "executor",
      "child": "exec-123",
      "handoff": "Task: execute the accepted plan\n",
      "deps": ["review"],
      "requires": [
        {"class": "agent", "name": "executor", "permission": "create"}
      ]
    }
  ]
}
"#;

    assert_eq!(
        record_child_handoff_to_parent_context(
            &session,
            "rev-123",
            "reviewer",
            "default",
            "Task: review the plan\n",
        ),
        Ok(())
    );
    assert_eq!(
        record_child_result_to_parent_context(
            &session,
            "rev-123",
            ChildContextStatus::Done,
            "Review accepted\n",
            "",
        ),
        Ok(())
    );
    assert_eq!(
        record_child_handoff_to_parent_context(
            &session,
            "exec-123",
            "executor",
            "default",
            "Task: execute the accepted plan\n",
        ),
        Ok(())
    );

    let completed = ok!(completed_agent_schedule_nodes_from_parent_context(
        &session,
        schedule,
        "planner_t",
        &policy,
        &["plan"]
    ));
    assert_eq!(completed, ["plan", "review"]);

    let ready = ok!(ready_agent_schedule_nodes(
        schedule,
        "planner_t",
        &policy,
        &completed.iter().map(String::as_str).collect::<Vec<_>>()
    ));
    assert_eq!(ready.len(), 1);
    let Some(node) = ready.first() else {
        return;
    };
    assert_eq!(node.id(), "execute");
}

#[test]
fn agent_schedule_completion_rejects_unknown_local_completion_and_symlink_child_status() {
    let root = clean_test_dir("agent-schedule-completed-bad");
    let outside = clean_test_dir("agent-schedule-completed-bad-outside");
    let session = root.join("default");
    create_complete_session_layout(&session);
    let policy = ok!(PolicyV0::parse("allow planner_t agent:reviewer create\n"));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "rev-123",
      "handoff": "Task: review the plan\n",
      "max_steps": 8,
      "requires": [
        {"class": "agent", "name": "reviewer", "permission": "create"}
      ]
    }
  ]
}
"#;

    let result = completed_agent_schedule_nodes_from_parent_context(
        &session,
        schedule,
        "planner_t",
        &policy,
        &["missing"],
    );
    assert!(matches!(
        result,
        Err(AgentScheduleRecordError::InvalidSchedule(_))
    ));

    assert_eq!(
        record_child_handoff_to_parent_context(
            &session,
            "rev-123",
            "reviewer",
            "default",
            "Task: review the plan\n",
        ),
        Ok(())
    );
    write_text_file(&outside.join("status"), "done\n");
    assert!(fs::remove_file(
        session
            .join("context")
            .join("child")
            .join("rev-123")
            .join("status")
    )
    .is_ok());
    assert!(symlink(
        outside.join("status"),
        session
            .join("context")
            .join("child")
            .join("rev-123")
            .join("status")
    )
    .is_ok());

    let result = completed_agent_schedule_nodes_from_parent_context(
        &session,
        schedule,
        "planner_t",
        &policy,
        &[],
    );
    assert_eq!(result, Err(AgentScheduleRecordError::CannotRecord));
}

#[test]
fn agent_schedule_completion_rejects_local_completion_for_delegated_node() {
    let root = clean_test_dir("agent-schedule-completed-local-delegated");
    let session = root.join("default");
    create_complete_session_layout(&session);
    let policy = ok!(PolicyV0::parse("allow planner_t agent:reviewer create\n"));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "rev-123",
      "handoff": "Task: review the plan\n",
      "max_steps": 8,
      "requires": [
        {"class": "agent", "name": "reviewer", "permission": "create"}
      ]
    }
  ]
}
"#;

    let result = completed_agent_schedule_nodes_from_parent_context(
        &session,
        schedule,
        "planner_t",
        &policy,
        &["review"],
    );

    assert!(matches!(
        result,
        Err(AgentScheduleRecordError::InvalidSchedule(_))
    ));
    let Err(AgentScheduleRecordError::InvalidSchedule(report)) = result else {
        return;
    };
    assert_eq!(
        report.issues(),
        &[AgentScheduleIssue::DelegatedCompletionRequiresChildResult {
            node: "review".to_owned()
        }]
    );
}

#[test]
fn agent_schedule_completion_rejects_done_status_from_conflicting_child_channel() {
    let root = clean_test_dir("agent-schedule-completed-conflicting-child");
    let session = root.join("default");
    create_complete_session_layout(&session);
    let policy = ok!(PolicyV0::parse("allow planner_t agent:reviewer create\n"));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "rev-123",
      "handoff": "Task: review the plan\n",
      "max_steps": 8,
      "requires": [
        {"class": "agent", "name": "reviewer", "permission": "create"}
      ]
    }
  ]
}
"#;

    assert_eq!(
        record_child_handoff_to_parent_context(
            &session,
            "rev-123",
            "reviewer",
            "default",
            "Different handoff\n",
        ),
        Ok(())
    );
    assert_eq!(
        record_child_result_to_parent_context(
            &session,
            "rev-123",
            ChildContextStatus::Done,
            "Review accepted\n",
            "",
        ),
        Ok(())
    );

    let result =
        completed_agent_schedule_nodes_from_parent_context(&session, schedule, "planner_t", &policy, &[]);

    assert_eq!(result, Err(AgentScheduleRecordError::CannotRecord));
}

#[test]
fn agent_schedule_completion_rejects_invalid_child_refs() {
    let root = clean_test_dir("agent-schedule-completed-invalid-refs");
    let session = root.join("default");
    create_complete_session_layout(&session);
    let policy = ok!(PolicyV0::parse("allow planner_t agent:reviewer create\n"));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "rev-123",
      "handoff": "Task: review the plan\n",
      "max_steps": 8,
      "requires": [
        {"class": "agent", "name": "reviewer", "permission": "create"}
      ]
    }
  ]
}
"#;

    assert_eq!(
        record_child_handoff_to_parent_context(
            &session,
            "rev-123",
            "reviewer",
            "default",
            "Task: review the plan\n",
        ),
        Ok(())
    );
    let child = session.join("context").join("child").join("rev-123");
    write_text_file(&child.join("status"), "done\n");
    write_text_file(&child.join("refs.jsonl"), "{\"id\":\"r1\",\"path\":\"../bad\"}\n");

    let result =
        completed_agent_schedule_nodes_from_parent_context(&session, schedule, "planner_t", &policy, &[]);

    assert_eq!(result, Err(AgentScheduleRecordError::CannotRecord));
}

#[test]
fn agent_schedule_advance_records_next_ready_handoffs_from_parent_state() {
    let root = clean_test_dir("agent-schedule-advance");
    let session = root.join("default");
    create_complete_session_layout(&session);
    let policy = ok!(PolicyV0::parse(
        "\
allow planner_t tool:fs.read execute
allow planner_t agent:reviewer create
allow planner_t agent:executor create
"
    ));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "plan",
      "kind": "dag",
      "agent": "planner",
      "requires": [
        {"class": "tool", "name": "fs.read", "permission": "execute"}
      ]
    },
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "rev-123",
      "handoff": "Task: review the plan\n",
      "deps": ["plan"],
      "max_steps": 8,
      "requires": [
        {"class": "agent", "name": "reviewer", "permission": "create"}
      ]
    },
    {
      "id": "execute",
      "kind": "dag",
      "agent": "executor",
      "child": "exec-123",
      "handoff": "Task: execute the accepted plan\n",
      "deps": ["review"],
      "requires": [
        {"class": "agent", "name": "executor", "permission": "create"}
      ]
    }
  ]
}
"#;

    let first = ok!(advance_agent_schedule_from_parent_context(
        &session,
        schedule,
        "planner_t",
        &policy,
        &["plan"]
    ));
    assert_schedule_advance(&first, &["plan"], &["review"]);
    assert_file_text(
        &session
            .join("context")
            .join("child")
            .join("rev-123")
            .join("handoff.md"),
        "Task: review the plan\n",
    );
    assert!(!session
        .join("context")
        .join("child")
        .join("exec-123")
        .exists());

    assert_eq!(
        record_child_result_to_parent_context(
            &session,
            "rev-123",
            ChildContextStatus::Done,
            "Review accepted\n",
            "",
        ),
        Ok(())
    );

    let second = ok!(advance_agent_schedule_from_parent_context(
        &session,
        schedule,
        "planner_t",
        &policy,
        &["plan"]
    ));
    assert_schedule_advance(&second, &["plan", "review"], &["execute"]);
    assert_file_text(
        &session
            .join("context")
            .join("child")
            .join("exec-123")
            .join("handoff.md"),
        "Task: execute the accepted plan\n",
    );
}

#[test]
fn agent_schedule_advance_does_not_rewrite_already_materialized_handoff() {
    let root = clean_test_dir("agent-schedule-advance-once");
    let session = root.join("default");
    create_complete_session_layout(&session);
    let policy = ok!(PolicyV0::parse("allow planner_t agent:reviewer create\n"));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "rev-123",
      "handoff": "Task: review the plan\n",
      "max_steps": 8,
      "requires": [
        {"class": "agent", "name": "reviewer", "permission": "create"}
      ]
    }
  ]
}
"#;

    let first = ok!(advance_agent_schedule_from_parent_context(
        &session,
        schedule,
        "planner_t",
        &policy,
        &[]
    ));
    assert_schedule_advance(&first, &[], &["review"]);

    let child = session.join("context").join("child").join("rev-123");
    write_text_file(&child.join("status"), "active\n");

    let second = ok!(advance_agent_schedule_from_parent_context(
        &session,
        schedule,
        "planner_t",
        &policy,
        &[]
    ));

    assert_schedule_advance(&second, &[], &[]);
    assert_file_text(&child.join("status"), "active\n");
    assert_file_text(&child.join("handoff.md"), "Task: review the plan\n");
}

#[test]
fn agent_schedule_advance_rejects_conflicting_materialized_handoff() {
    let root = clean_test_dir("agent-schedule-advance-conflict");
    let session = root.join("default");
    create_complete_session_layout(&session);
    let policy = ok!(PolicyV0::parse("allow planner_t agent:reviewer create\n"));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "rev-123",
      "handoff": "Task: review the plan\n",
      "max_steps": 8,
      "requires": [
        {"class": "agent", "name": "reviewer", "permission": "create"}
      ]
    }
  ]
}
"#;

    assert_eq!(
        record_child_handoff_to_parent_context(
            &session,
            "rev-123",
            "reviewer",
            "default",
            "Different handoff\n",
        ),
        Ok(())
    );

    let advanced =
        advance_agent_schedule_from_parent_context(&session, schedule, "planner_t", &policy, &[]);

    assert_eq!(advanced, Err(AgentScheduleRecordError::CannotRecord));
    assert_file_text(
        &session
            .join("context")
            .join("child")
            .join("rev-123")
            .join("handoff.md"),
        "Different handoff\n",
    );
}

#[test]
fn agent_schedule_advance_rejects_malformed_materialized_status() {
    let root = clean_test_dir("agent-schedule-advance-bad-status");
    let session = root.join("default");
    create_complete_session_layout(&session);
    let policy = ok!(PolicyV0::parse("allow planner_t agent:reviewer create\n"));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "rev-123",
      "handoff": "Task: review the plan\n",
      "max_steps": 8,
      "requires": [
        {"class": "agent", "name": "reviewer", "permission": "create"}
      ]
    }
  ]
}
"#;

    assert_eq!(
        record_child_handoff_to_parent_context(
            &session,
            "rev-123",
            "reviewer",
            "default",
            "Task: review the plan\n",
        ),
        Ok(())
    );
    write_text_file(
        &session
            .join("context")
            .join("child")
            .join("rev-123")
            .join("status"),
        "waiting\n",
    );

    let advanced =
        advance_agent_schedule_from_parent_context(&session, schedule, "planner_t", &policy, &[]);

    assert_eq!(advanced, Err(AgentScheduleRecordError::CannotRecord));
}

fn assert_schedule_advance(
    advance: &AgentScheduleAdvance,
    completed: &[&str],
    handoff_nodes: &[&str],
) {
    assert_eq!(
        advance.completed_nodes(),
        completed
            .iter()
            .map(|node| (*node).to_owned())
            .collect::<Vec<_>>()
            .as_slice()
    );
    assert_eq!(
        advance
            .handoffs()
            .iter()
            .map(AgentScheduleChildHandoff::node)
            .collect::<Vec<_>>(),
        handoff_nodes
    );
}

#[test]
fn agent_schedule_recorder_writes_parent_context_plan() {
    let root = clean_test_dir("agent-schedule-record");
    let session = root.join("default");
    create_complete_session_layout(&session);
    let policy = ok!(PolicyV0::parse(
        "\
allow planner_t tool:fs.read execute
allow planner_t agent:reviewer create
"
    ));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "plan",
      "kind": "dag",
      "agent": "planner",
      "requires": [
        {"class": "tool", "name": "fs.read", "permission": "execute"}
      ]
    },
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "rev-123",
      "handoff": "Task: review the plan\n",
      "deps": ["plan"],
      "max_steps": 4,
      "requires": [
        {"class": "agent", "name": "reviewer", "permission": "create"}
      ]
    }
  ]
}
"#;

    let result =
        record_agent_schedule_to_parent_context(&session, schedule, "planner_t", &policy);

    assert_eq!(result, Ok(()));
    assert_file_text(
        &session.join("context").join("plan.json"),
        &format!("{}\n", schedule.trim_end()),
    );
    assert!(validate_context_pack_source("context/plan.json").is_ok());
}

#[test]
fn agent_schedule_recorder_rejects_invalid_schedule_without_replacing_plan() {
    let root = clean_test_dir("agent-schedule-record-invalid");
    let session = root.join("default");
    create_complete_session_layout(&session);
    write_text_file(&session.join("context").join("plan.json"), "{\"old\":true}\n");
    let policy = ok!(PolicyV0::parse("allow planner_t tool:fs.read execute\n"));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "max_steps": 8,
      "requires": [
        {"class": "tool", "name": "shell.exec", "permission": "execute"}
      ]
    }
  ]
}
"#;

    let result =
        record_agent_schedule_to_parent_context(&session, schedule, "planner_t", &policy);

    assert!(matches!(
        result,
        Err(AgentScheduleRecordError::InvalidSchedule(_))
    ));
    let Err(AgentScheduleRecordError::InvalidSchedule(report)) = result else {
        return;
    };
    assert!(report.issues().contains(&AgentScheduleIssue::PermissionNotGranted {
        node: "review".to_owned(),
        class: "tool".to_owned(),
        name: "shell.exec".to_owned(),
        permission: "execute".to_owned()
    }));
    assert_eq!(
        AgentScheduleRecordError::InvalidText.errno(),
        "EINVAL"
    );
    assert_file_text(&session.join("context").join("plan.json"), "{\"old\":true}\n");
}

#[test]
fn agent_schedule_recorder_rejects_symlink_parent_context() {
    let root = clean_test_dir("agent-schedule-record-symlink");
    let session = root.join("default");
    let outside = clean_test_dir("agent-schedule-record-symlink-outside");
    create_complete_session_layout(&session);
    assert!(fs::remove_dir_all(session.join("context")).is_ok());
    assert!(fs::create_dir_all(outside.join("context")).is_ok());
    assert!(symlink(outside.join("context"), session.join("context")).is_ok());
    let policy = ok!(PolicyV0::parse("allow planner_t tool:fs.read execute\n"));
    let schedule = r#"{"version":1,"mode":"dag-react","nodes":[{"id":"plan","kind":"dag","agent":"planner","requires":[{"class":"tool","name":"fs.read","permission":"execute"}]}]}"#;

    let result =
        record_agent_schedule_to_parent_context(&session, schedule, "planner_t", &policy);

    assert_eq!(result, Err(AgentScheduleRecordError::MissingParentSession));
    assert!(!outside.join("context").join("plan.json").exists());
}
