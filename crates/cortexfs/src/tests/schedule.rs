use super::*;

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
    Ok((root, session, policy, schedule))
}

mod advance;
mod completion;
mod handoffs;
mod validation;
