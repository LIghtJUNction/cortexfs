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

    let result = ready_agent_schedule_nodes(schedule, "planner_t", &policy, &["missing"]);
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
    let schedule = delegated_schedule("review");

    let handoffs = ok!(ready_agent_schedule_child_handoffs(
        &schedule,
        "planner_t",
        &policy,
        &[],
        "default"
    ));
    assert_eq!(handoffs, []);

    let handoffs = ok!(ready_agent_schedule_child_handoffs(
        &schedule,
        "planner_t",
        &policy,
        &["plan"],
        "default"
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
        &schedule,
        "planner_t",
        &policy,
        &["plan", "review"],
        "default"
    ));
    assert_eq!(handoffs.len(), 1);
    let Some(handoff) = handoffs.first() else {
        return;
    };
    assert_eq!(handoff.node(), "execute");
    assert_eq!(handoff.session(), "run-123");
}
use super::*;
