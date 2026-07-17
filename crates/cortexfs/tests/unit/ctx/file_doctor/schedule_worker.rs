const REPLACED_WORKER_PLAN: &str = r#"{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {"id": "local", "kind": "dag", "agent": "coder"}
  ]
}
"#;
const WORKER_PLAN_ABI_PATH: &str = "home/1000/agent/coder/session/default/context/plan.json";

fn assert_worker_claimed_active(root: &Path, child: &Path) {
    assert_eq!(schedule_claim_worker(root), Ok(()));
    assert!(matches!(
        fs::read_to_string(child.join("status")).as_deref(),
        Ok("active\n")
    ));
    let active_inode = fs::metadata(child.join("status"))
        .map(|metadata| metadata.ino())
        .unwrap_or_default();
    assert_eq!(schedule_claim_worker(root), Ok(()));
    assert_eq!(
        fs::metadata(child.join("status"))
            .map(|metadata| metadata.ino())
            .unwrap_or_default(),
        active_inode
    );
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
            parent_run: None,
            model: "api.lmm.best/gpt-5.3-codex-spark".to_owned(),
            life: "temp".to_owned(),
            agent_status: "idle".to_owned(),
            ppid: None,
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
            path: WORKER_PLAN_ABI_PATH.to_owned(),
            child: "work-123".to_owned(),
        },
    )
}

fn schedule_status_worker(root: &Path) -> Result<(), CliError> {
    schedule_command(
        root,
        &ScheduleArgs::Status {
            path: WORKER_PLAN_ABI_PATH.to_owned(),
            done: Vec::new(),
        },
    )
}

fn schedule_result_worker(
    root: &Path,
    status: ChildContextStatus,
    result: &str,
    refs_jsonl: &str,
) -> Result<(), CliError> {
    schedule_command(
        root,
        &ScheduleArgs::Result {
            path: WORKER_PLAN_ABI_PATH.to_owned(),
            child: "work-123".to_owned(),
            status,
            result: result.to_owned(),
            refs_jsonl: refs_jsonl.to_owned(),
        },
    )
}

fn worker_plan_path(child: &Path) -> PathBuf {
    child
        .parent()
        .and_then(Path::parent)
        .unwrap_or(child)
        .join("plan.json")
}

fn child_terminal_snapshot(child: &Path) -> [(String, u64); 3] {
    ["status", "result.md", "refs.jsonl"].map(|file| {
        let path = child.join(file);
        (
            fs::read_to_string(&path).unwrap_or_default(),
            fs::metadata(path)
                .map(|metadata| metadata.ino())
                .unwrap_or_default(),
        )
    })
}

fn replace_file_for_schedule_race(path: &Path, value: &str) -> Result<(), CliError> {
    let temporary = path.with_file_name(".plan.race.json");
    write_text_file(&temporary, value);
    fs::rename(&temporary, path).map_err(|error| {
        CliError::unavailable(format!("cannot replace schedule race fixture: {error}"))
    })
}

fn replace_handoff_for_schedule_race(path: &Path, value: &str) -> Result<(), CliError> {
    let temporary = path.with_file_name(".handoff.race.md");
    write_text_file(&temporary, value);
    fs::rename(&temporary, path).map_err(|error| {
        CliError::unavailable(format!("cannot replace handoff race fixture: {error}"))
    })
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
    let ensured = ensure_reference_tree(root);
    assert!(ensured.is_ok(), "{ensured:?}");
    enable_dynamic_worker_fixture(root);
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");
    let session = fixture_path(
        root,
        &["home", "1000", "agent", "coder", "session", "default"],
    );
    create_complete_session_layout(&session);
    write_worker_schedule_plan(&session);
    assert!(
        schedule_command(
            root,
            &ScheduleArgs::Advance {
                path: WORKER_PLAN_ABI_PATH.to_owned(),
                done: Vec::new(),
            },
        )
        .is_ok(),
        "{test_name}"
    );
    session.join("context").join("child").join("work-123")
}

fn assert_worker_schedule_status(root: &Path, status: &str) {
    let child_parent = schedule_handoff_agent_details(root, "worker")
        .map_or_else(|_| "-".to_owned(), |(_, _, parent)| parent);
    assert_eq!(
        assert_schedule_status_rows(
            root,
            &[&format!(
                "implement\treact\tworker\twork-123\tdefault\tapi.lmm.best/gpt-5.3-codex-spark\ttemp\tworker\t{child_parent}\t{status}"
            )],
        ),
        Ok(())
    );
}

fn assert_schedule_status_rows(root: &Path, expected: &[&str]) -> Result<(), CliError> {
    let schedule = load_schedule_context(root, WORKER_PLAN_ABI_PATH, "status")?;
    let lines = schedule_status_lines(root, &schedule, &[])?;
    assert_eq!(lines, expected);
    Ok(())
}
