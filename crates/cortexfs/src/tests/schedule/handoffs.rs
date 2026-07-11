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
    assert!(
        report
            .issues()
            .contains(&AgentScheduleIssue::DuplicateChild {
                child: "shared-child".to_owned()
            })
    );
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

    let handoffs = ok!(
        record_ready_agent_schedule_child_handoffs_to_parent_context(
            &session,
            schedule,
            "planner_t",
            &policy,
            &["plan"]
        )
    );

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
    assert!(
        !session
            .join("context")
            .join("child")
            .join("exec-123")
            .exists()
    );
}
use super::*;
