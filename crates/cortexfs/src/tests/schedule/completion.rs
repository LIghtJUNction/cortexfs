#[test]
fn agent_schedule_completion_derives_done_delegated_nodes_from_child_status() {
    let root = clean_test_dir("agent-schedule-completed-from-child");
    let session = root.join("default");
    create_complete_session_layout(&session);
    let policy = ok!(PolicyV0::parse(
        "\
allow planner_t tool:fs.read execute
allow planner_t agent:reviewer create
allow planner_t agent:worker create
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
      "child": "exec-123",
      "handoff": "Task: execute the accepted plan\n",
      "deps": ["review"],
      "requires": [
        {"class": "agent", "name": "worker", "permission": "create"}
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
            "worker",
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
    assert!(
        fs::remove_file(
            session
                .join("context")
                .join("child")
                .join("rev-123")
                .join("status")
        )
        .is_ok()
    );
    assert!(
        symlink(
            outside.join("status"),
            session
                .join("context")
                .join("child")
                .join("rev-123")
                .join("status")
        )
        .is_ok()
    );

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
    write_text_file(
        &child.join("refs.jsonl"),
        "{\"id\":\"r1\",\"path\":\"../bad\"}\n",
    );

    let result = completed_agent_schedule_nodes_from_parent_context(
        &session,
        schedule,
        "planner_t",
        &policy,
        &[],
    );

    assert_eq!(result, Err(AgentScheduleRecordError::CannotRecord));
}
use super::*;
