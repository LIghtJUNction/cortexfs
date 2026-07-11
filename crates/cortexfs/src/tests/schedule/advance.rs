#[test]
fn agent_schedule_advance_records_next_ready_handoffs_from_parent_state() {
    let root = clean_test_dir("agent-schedule-advance");
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
    assert!(
        !session
            .join("context")
            .join("child")
            .join("exec-123")
            .exists()
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
            .join("agent"),
        "worker\n",
    );
    assert_file_text(
        &session
            .join("context")
            .join("child")
            .join("exec-123")
            .join("session"),
        "default\n",
    );
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
fn agent_schedule_advance_defaults_child_session_to_parent_session() {
    let root = clean_test_dir("agent-schedule-parent-session");
    let session = root.join("feature");
    create_complete_session_layout(&session);
    let policy = ok!(PolicyV0::parse("allow planner_t agent:worker create\n"));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "implement",
      "kind": "react",
      "child": "work-123",
      "handoff": "Task: implement\n",
      "max_steps": 8,
      "requires": [
        {"class": "agent", "name": "worker", "permission": "create"}
      ]
    }
  ]
}
"#;

    let advance = ok!(advance_agent_schedule_from_parent_context(
        &session,
        schedule,
        "planner_t",
        &policy,
        &[]
    ));
    assert_schedule_advance(&advance, &[], &["implement"]);
    assert_file_text(
        &session
            .join("context")
            .join("child")
            .join("work-123")
            .join("session"),
        "feature\n",
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

    let result = record_agent_schedule_to_parent_context(&session, schedule, "planner_t", &policy);

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
    write_text_file(
        &session.join("context").join("plan.json"),
        "{\"old\":true}\n",
    );
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

    let result = record_agent_schedule_to_parent_context(&session, schedule, "planner_t", &policy);

    assert!(matches!(
        result,
        Err(AgentScheduleRecordError::InvalidSchedule(_))
    ));
    let Err(AgentScheduleRecordError::InvalidSchedule(report)) = result else {
        return;
    };
    assert!(
        report
            .issues()
            .contains(&AgentScheduleIssue::PermissionNotGranted {
                node: "review".to_owned(),
                class: "tool".to_owned(),
                name: "shell.exec".to_owned(),
                permission: "execute".to_owned()
            })
    );
    assert_eq!(AgentScheduleRecordError::InvalidText.errno(), "EINVAL");
    assert_file_text(
        &session.join("context").join("plan.json"),
        "{\"old\":true}\n",
    );
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

    let result = record_agent_schedule_to_parent_context(&session, schedule, "planner_t", &policy);

    assert_eq!(result, Err(AgentScheduleRecordError::MissingParentSession));
    assert!(!outside.join("context").join("plan.json").exists());
}
use super::*;
