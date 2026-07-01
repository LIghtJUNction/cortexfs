fn assert_worker_claimed_active(root: &Path, child: &Path) {
    assert_eq!(schedule_claim_worker(root), Ok(()));
    assert!(matches!(
        fs::read_to_string(child.join("status")).as_deref(),
        Ok("active\n")
    ));
    assert_eq!(schedule_claim_worker(root), Ok(()));
    let active_wait = agent_wait(root, "coder", Some("default"), "work-123");
    assert!(matches!(
        active_wait,
        Err(ref error)
            if error.code == 69
                && error.message == "child work-123 is not terminal: active"
    ));
    assert_worker_child_row_status(root, "active");
}

fn assert_worker_child_row_status(root: &Path, status: &str) {
    let rows = agent_child_rows(root, "coder", Some("default"));
    assert!(matches!(
        rows,
        Ok(ref rows) if rows.contains(&AgentChildRow {
            child: "work-123".to_owned(),
            status: status.to_owned(),
            agent: "worker".to_owned(),
            session: "default".to_owned(),
            parent_session: None,
            model: "api.lmm.best/gpt-5.3-codex-spark".to_owned(),
            life: "temp".to_owned(),
            agent_status: "idle".to_owned(),
            pid: None,
        })
    ));
}

fn assert_worker_claim_rejects_terminal(root: &Path) {
    assert!(matches!(
        schedule_claim_worker(root),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid child context: invalid status transition"
    ));
}

fn schedule_claim_worker(root: &Path) -> Result<(), CliError> {
    schedule_command(
        root,
        &ScheduleArgs::Claim {
            path: "home/1000/agent/coder/session/default/context/plan.json".to_owned(),
            child: "work-123".to_owned(),
        },
    )
}

fn schedule_status_worker(root: &Path) -> Result<(), CliError> {
    schedule_command(
        root,
        &ScheduleArgs::Status {
            path: "home/1000/agent/coder/session/default/context/plan.json".to_owned(),
            done: Vec::new(),
        },
    )
}

fn write_worker_schedule_plan(session: &Path) {
    write_text_file(
        &session.join("context").join("plan.json"),
        r#"{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "implement",
      "kind": "react",
      "child": "work-123",
      "handoff": "Task: implement the accepted plan\n",
      "max_steps": 8,
      "requires": [
        {"class":"agent","name":"worker","permission":"create"}
      ]
    }
  ]
}
"#,
    );
}

fn create_pending_worker_handoff(root: &Path, test_name: &str) -> PathBuf {
    assert!(ensure_v1_reference_tree(root).is_ok());
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");
    let session = fixture_path(
        root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );
    create_complete_session_layout(&session);
    write_worker_schedule_plan(&session);
    assert!(
        schedule_command(
            root,
            &ScheduleArgs::Advance {
                path: "home/1000/agent/coder/session/default/context/plan.json".to_owned(),
                done: Vec::new(),
            },
        )
        .is_ok(),
        "{test_name}"
    );
    session.join("context").join("child").join("work-123")
}

fn assert_worker_schedule_status(root: &Path, status: &str) {
    assert_eq!(
        assert_schedule_status_rows(
            root,
            &[&format!(
                "implement\treact\tworker\twork-123\tdefault\tapi.lmm.best/gpt-5.3-codex-spark\ttemp\tworker\t{status}"
            )],
        ),
        Ok(())
    );
}

fn assert_schedule_status_rows(root: &Path, expected: &[&str]) -> Result<(), CliError> {
    let schedule = load_schedule_context(
        root,
        "home/1000/agent/coder/session/default/context/plan.json",
        "status",
    )?;
    let lines = schedule_status_lines(root, &schedule, &[])?;
    assert_eq!(lines, expected);
    Ok(())
}

fn assert_worker_wait_status(root: &Path, status: ChildContextStatus, result: &str, code: u8) {
    let recorded = schedule_command(
        root,
        &ScheduleArgs::Result {
            path: "home/1000/agent/coder/session/default/context/plan.json".to_owned(),
            child: "work-123".to_owned(),
            status,
            result: result.to_owned(),
            refs_jsonl: String::new(),
        },
    );

    assert_eq!(recorded, Ok(()));
    assert_eq!(
        agent_wait(root, "coder", Some("default"), "work-123"),
        Ok(ExitCode::from(code))
    );
}
