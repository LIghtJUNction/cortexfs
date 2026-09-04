use super::*;

const REVIEW_ONLY_SCHEDULE: &str = r#"
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

const THREE_STAGE_SCHEDULE: &str = r#"
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

fn three_stage_schedule_fixture(
    name: &str,
) -> Result<(TestDir, PathBuf, PolicyV0, &'static str), PolicyError> {
    let root = clean_test_dir(name);
    let session = root.join("default");
    create_complete_session_layout(&session);
    let policy = PolicyV0::parse(
        "\
allow planner_t tool:fs.read execute
allow planner_t agent:reviewer create
allow planner_t agent:worker create
",
    )?;
    Ok((root, session, policy, THREE_STAGE_SCHEDULE))
}

fn review_only_schedule_fixture(
    name: &str,
) -> Result<(TestDir, PathBuf, PolicyV0, &'static str), PolicyError> {
    let root = clean_test_dir(name);
    let session = root.join("default");
    create_complete_session_layout(&session);
    let policy = PolicyV0::parse("allow planner_t agent:reviewer create\n")?;
    Ok((root, session, policy, REVIEW_ONLY_SCHEDULE))
}

fn delegated_schedule(execute_dependency: &str) -> String {
    format!(
        r#"
{{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {{
      "id": "plan",
      "kind": "dag",
      "agent": "planner",
      "requires": [
        {{"class": "tool", "name": "fs.read", "permission": "execute"}}
      ]
    }},
    {{
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "rev-123",
      "handoff": "Task: review the plan\n",
      "deps": ["plan"],
      "max_steps": 8,
      "requires": [
        {{"class": "agent", "name": "reviewer", "permission": "create"}}
      ]
    }},
    {{
      "id": "execute",
      "kind": "dag",
      "agent": "executor",
      "child": "exec-123",
      "session": "run-123",
      "handoff": "Task: execute the accepted plan\n",
      "deps": ["{execute_dependency}"],
      "requires": [
        {{"class": "agent", "name": "executor", "permission": "create"}}
      ]
    }}
  ]
}}
"#
    )
}

fn complete_review(session: &Path) {
    let receipt = ok!(crate::child_handoff_receipt(
        &session.join("context").join("child").join("rev-123")
    ));
    assert_eq!(
        claim_child_handoff_active(&receipt, "reviewer", "default", None),
        Ok(())
    );
    assert_eq!(
        record_child_result_to_parent_context(
            session,
            "rev-123",
            ChildContextStatus::Done,
            "Review accepted\n",
            "",
        ),
        Ok(())
    );
}

mod advance;
mod completion;
mod handoffs;
mod validation;
