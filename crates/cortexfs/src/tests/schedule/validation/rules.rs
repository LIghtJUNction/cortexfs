#[test]
fn agent_schedule_accepts_bounded_dag_react_plan_with_parent_permissions() {
    let policy = ok!(PolicyV0::parse(
        "\
allow planner_t tool:fs.read execute
allow planner_t model:openai/gpt-5.6 use
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
        {"class": "model", "name": "openai/gpt-5.6", "permission": "use"}
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
fn agent_schedule_defaults_delegated_nodes_to_worker_agent() {
    let policy = ok!(PolicyV0::parse(
        "\
allow planner_t agent:worker create
allow planner_t tool:fs.read execute
"
    ));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "implement",
      "kind": "react",
      "child": "work-123",
      "handoff": "Task: implement the next slice\n",
      "max_steps": 8,
      "requires": [
        {"class": "tool", "name": "fs.read", "permission": "execute"}
      ]
    }
  ]
}
"#;

    let nodes = ok!(ready_agent_schedule_nodes(
        schedule,
        "planner_t",
        &policy,
        &[]
    ));
    assert_eq!(nodes.len(), 1);
    let Some(node) = nodes.first() else {
        return;
    };
    assert_eq!(node.agent(), "worker");
    assert_eq!(node.child(), Some("work-123"));
    assert_eq!(node.child_session(), None);

    let ready = ok!(ready_agent_schedule_child_handoffs(
        schedule,
        "planner_t",
        &policy,
        &[],
        "feature"
    ));
    let Some(handoff) = ready.first() else {
        return;
    };
    assert_eq!(handoff.agent(), "worker");
    assert_eq!(handoff.child(), "work-123");
    assert_eq!(handoff.session(), "feature");
}

#[test]
fn agent_schedule_requires_worker_create_for_implicit_worker_handoff() {
    let policy = ok!(PolicyV0::parse("allow planner_t tool:fs.read execute\n"));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "implement",
      "kind": "react",
      "child": "work-123",
      "handoff": "Task: implement the next slice\n",
      "max_steps": 8,
      "requires": [
        {"class": "tool", "name": "fs.read", "permission": "execute"}
      ]
    }
  ]
}
"#;

    let report = inspect_agent_schedule_json(schedule, "planner_t", &policy);
    assert_eq!(
        report.issues(),
        &[AgentScheduleIssue::PermissionNotGranted {
            node: "implement".to_owned(),
            class: "agent".to_owned(),
            name: "worker".to_owned(),
            permission: "create".to_owned()
        }]
    );
}

#[test]
fn agent_schedule_still_requires_agent_for_parent_local_nodes() {
    let policy = ok!(PolicyV0::parse(""));
    let schedule = r#"
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {"id": "plan", "kind": "dag"}
  ]
}
"#;

    let report = inspect_agent_schedule_json(schedule, "planner_t", &policy);
    assert_eq!(
        report.issues(),
        &[AgentScheduleIssue::InvalidField {
            node: Some("plan".to_owned()),
            field: "agent".to_owned(),
            value: String::new()
        }]
    );
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
    assert!(
        report
            .issues()
            .contains(&AgentScheduleIssue::UnknownDependency {
                node: "review".to_owned(),
                dependency: "missing".to_owned()
            })
    );
    assert!(
        report
            .issues()
            .contains(&AgentScheduleIssue::DependencyCycle {
                node: "plan".to_owned()
            })
    );
    assert!(
        report
            .issues()
            .contains(&AgentScheduleIssue::InvalidReactBound {
                node: "review".to_owned()
            })
    );
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
fn agent_schedule_rejects_too_many_nodes_before_dependency_inspection() {
    let policy = ok!(PolicyV0::parse(""));
    let nodes = (0..=MAX_AGENT_SCHEDULE_NODES)
        .map(|index| {
            format!(r#"{{"id":"n-{index}","kind":"dag","agent":"planner","deps":["missing"]}}"#)
        })
        .collect::<Vec<_>>()
        .join(",");
    let schedule = format!(
        r#"{{
  "version": 1,
  "mode": "dag-react",
  "nodes": [{nodes}]
}}"#
    );

    let report = inspect_agent_schedule_json(&schedule, "planner_t", &policy);
    assert_eq!(report.issues(), &[AgentScheduleIssue::InvalidNodes]);
}

#[test]
fn agent_schedule_accepts_long_acyclic_dependency_chain_without_recursion() {
    let policy = ok!(PolicyV0::parse(""));
    let nodes = (0..MAX_AGENT_SCHEDULE_NODES)
        .map(|index| {
            if index + 1 == MAX_AGENT_SCHEDULE_NODES {
                format!(r#"{{"id":"n-{index}","kind":"dag","agent":"planner"}}"#)
            } else {
                format!(
                    r#"{{"id":"n-{index}","kind":"dag","agent":"planner","deps":["n-{}"]}}"#,
                    index + 1
                )
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    let schedule = format!(
        r#"{{
  "version": 1,
  "mode": "dag-react",
  "nodes": [{nodes}]
}}"#
    );

    let report = inspect_agent_schedule_json(&schedule, "planner_t", &policy);
    assert_eq!(report.issues(), &[]);
}

#[test]
fn agent_schedule_ready_nodes_follow_dag_dependencies_in_plan_order() {
    let policy = ok!(PolicyV0::parse(
        "\
allow planner_t tool:fs.read execute
allow planner_t agent:reviewer create
allow planner_t agent:worker create
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
    assert_eq!(review.child_session(), None);
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
use super::*;

struct FixedSchedulePolicy(bool);

impl crate::PolicyEvaluator for FixedSchedulePolicy {
    fn evaluate(
        &self,
        _subject: &str,
        _class: PolicyObjectClass,
        _name: &str,
        _permission: PolicyPermission,
    ) -> bool {
        self.0
    }
}

#[test]
fn schedule_permissions_accept_replaceable_policy_evaluators() {
    let schedule = r#"{
      "version": 1,
      "mode": "dag-react",
      "nodes": [{
        "id": "read",
        "kind": "dag",
        "agent": "planner",
        "requires": [
          {"class": "tool", "name": "fs.read", "permission": "execute"}
        ]
      }]
    }"#;
    assert!(inspect_agent_schedule_json(schedule, "custom_t", &FixedSchedulePolicy(true)).is_ok());
    assert!(
        !inspect_agent_schedule_json(schedule, "custom_t", &FixedSchedulePolicy(false)).is_ok()
    );
}
