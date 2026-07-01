#[test]
fn agent_stop_host_fallback_removes_temp_worker_object_after_cancellation() -> Result<(), CliError> {
    let root = clean_test_dir("ctx-agent-stop-temp-worker-cleanup");
    create_agent_fixture(&root, "coder", "agent:base", "busy", "100");
    create_agent_fixture(&root, "worker", "agent:coder session:default run:r1", "busy", "101");
    write_text_file(&root.join("agent/coder.d/log"), "");
    write_text_file(&root.join("agent/worker.d/log"), "");
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");
    let socket = root.join("agent/worker.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket)
        .map_err(|error| CliError::unavailable(format!("cannot bind socket: {error}")))?;
    let session = ctx_home(&root)
        .unwrap_or_default()
        .join("agent/coder/session/default");
    create_complete_session_layout(&session);
    let child = session.join("context/child/work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "active\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(&child.join("result.md"), "");
    write_text_file(&child.join("refs.jsonl"), "");

    assert_eq!(agent_stop(&root, "coder"), Ok(ExitCode::SUCCESS));

    assert!(!root.join("agent/worker").exists());
    assert!(!root.join("agent/worker.sock").exists());
    assert!(!root.join("agent/worker.d").exists());
    assert_eq!(
        fs::read_to_string(child.join("status")).unwrap_or_default(),
        "cancelled\n"
    );
    assert_eq!(
        fs::read_to_string(child.join("result.md")).unwrap_or_default(),
        "Child agent `worker` cancelled because the parent agent stopped.\n"
    );
    let rows = agent_child_rows(&root, "coder", Some("default"))?;
    assert!(rows.contains(&AgentChildRow {
            child: "work-123".to_owned(),
            status: "cancelled".to_owned(),
            agent: "worker".to_owned(),
            session: "default".to_owned(),
            parent_session: None,
            model: "api.lmm.best/gpt-5.3-codex-spark".to_owned(),
            life: "unknown".to_owned(),
            agent_status: "unknown".to_owned(),
            pid: None,
        }));
    assert_eq!(
        agent_wait(&root, "coder", Some("default"), "work-123"),
        Ok(ExitCode::from(130))
    );
    Ok(())
}
