#[test]
fn agent_schedule_completion_derives_done_delegated_nodes_from_child_status() {
    let (_root, session, policy, schedule) = ok!(three_stage_schedule_fixture(
        "agent-schedule-completed-from-child"
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
    complete_review(&session);
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
    let outside = clean_test_dir("agent-schedule-completed-bad-outside");
    let (_root, session, policy, schedule) =
        ok!(review_only_schedule_fixture("agent-schedule-completed-bad"));

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
    let child_status = session
        .join("context")
        .join("child")
        .join("rev-123")
        .join("status");
    assert!(fs::remove_file(&child_status).is_ok());
    assert!(symlink(outside.join("status"), &child_status).is_ok());

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
    let (_root, session, policy, schedule) = ok!(review_only_schedule_fixture(
        "agent-schedule-completed-local-delegated"
    ));

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
    let (_root, session, policy, schedule) = ok!(review_only_schedule_fixture(
        "agent-schedule-completed-conflicting-child"
    ));

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
    complete_review(&session);

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
    let (_root, session, policy, schedule) = ok!(review_only_schedule_fixture(
        "agent-schedule-completed-invalid-refs"
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
