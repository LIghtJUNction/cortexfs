#[test]
fn agent_wait_reaps_active_child_when_backing_worker_is_dead() {
    let root = clean_test_dir("ctx-agent-wait-reaps-dead-worker");
    let pid = std::process::id().to_string();
    let ensured = ensure_v1_reference_tree(&root);
    assert!(ensured.is_ok());
    write_text_file(&root.join("agent/coder.d/pid"), &format!("{pid}\n"));
    let session = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );
    create_complete_session_layout(&session);
    let child = session.join("context").join("child").join("work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "active\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(&child.join("result.md"), "");
    write_text_file(&child.join("refs.jsonl"), "");
    write_text_file(
        &root.join("agent/worker.d/parent"),
        "agent:coder session:default run:r1\n",
    );
    write_text_file(&root.join("agent/worker.d/status"), "dead\n");
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");
    write_text_file(&root.join("agent/worker.d/pid"), "\n");

    let rows = agent_child_rows(&root, "coder", Some("default"));
    assert!(matches!(
        rows,
        Ok(ref rows) if rows.contains(&AgentChildRow {
            child: "work-123".to_owned(),
            status: "cancelled".to_owned(),
            agent: "worker".to_owned(),
            session: "default".to_owned(),
            parent_session: Some("default".to_owned()),
            parent_run: Some("r1".to_owned()),
            model: "api.lmm.best/gpt-5.3-codex-spark".to_owned(),
            life: "temp".to_owned(),
            agent_status: "dead".to_owned(),
            ppid: Some(pid),
            pid: None,
        })
    ));
    assert_eq!(
        agent_wait(&root, "coder", Some("default"), "work-123"),
        Ok(ExitCode::from(130))
    );
    assert!(matches!(
        fs::read_to_string(child.join("status")).as_deref(),
        Ok("cancelled\n")
    ));
    assert!(matches!(
        fs::read_to_string(child.join("result.md")).as_deref(),
        Ok("Child agent `worker` session `default` is dead.\n")
    ));
    assert!(root.join("agent/worker.d").exists());
}

#[test]
fn agent_wait_reaps_worker_prefix_child_with_spark_default() {
    for agent in ["worker-fast", "executor-fast"] {
        let root = clean_test_dir(&format!("ctx-agent-wait-{agent}-spark"));
        assert!(ensure_v1_reference_tree(&root).is_ok());
        let session = fixture_path(
            &root,
            &[
                "home", "1000", "agent", "coder", "session", "default",
            ],
        );
        create_complete_session_layout(&session);
        let child = session.join("context").join("child").join("work-fast");
        assert!(fs::create_dir_all(child.join("artifact")).is_ok());
        write_text_file(&child.join("agent"), &format!("{agent}\n"));
        write_text_file(&child.join("session"), "default\n");
        write_text_file(&child.join("status"), "active\n");
        write_text_file(&child.join("handoff.md"), "Task: implement.\n");
        write_text_file(&child.join("result.md"), "");
        write_text_file(&child.join("refs.jsonl"), "");
        let control = root.join("agent").join(format!("{agent}.d"));
        assert!(fs::create_dir_all(&control).is_ok());
        write_text_file(&control.join("parent"), "agent:coder session:default run:r1\n");
        write_text_file(&control.join("status"), "dead\n");
        write_text_file(&control.join("life"), "temp\n");
        write_text_file(&control.join("pid"), "\n");

        let rows = agent_child_rows(&root, "coder", Some("default"));
        assert!(matches!(
            rows,
            Ok(ref rows) if rows.contains(&AgentChildRow {
                child: "work-fast".to_owned(),
                status: "cancelled".to_owned(),
                agent: agent.to_owned(),
                session: "default".to_owned(),
                parent_session: Some("default".to_owned()),
                parent_run: Some("r1".to_owned()),
                model: "api.lmm.best/gpt-5.3-codex-spark".to_owned(),
                life: "temp".to_owned(),
                agent_status: "dead".to_owned(),
                ppid: None,
                pid: None,
            })
        ));
        assert_eq!(
            agent_wait(&root, "coder", Some("default"), "work-fast"),
            Ok(ExitCode::from(130))
        );
        assert!(matches!(
            fs::read_to_string(child.join("result.md")).as_deref(),
            Ok(result) if result == format!("Child agent `{agent}` session `default` is dead.\n")
        ));
        assert!(!control.exists());
    }
}

#[test]
fn agent_child_rows_default_missing_worker_model_to_spark() {
    let root = clean_test_dir("ctx-child-row-missing-worker-model");
    let ensured = ensure_v1_reference_tree(&root);
    assert!(ensured.is_ok());
    enable_dynamic_worker_fixture(&root);
    assert!(fs::remove_file(root.join("agent/worker.d/model")).is_ok());
    write_text_file(&root.join("agent/worker.d/status"), "idle\n");
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");
    let session = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );
    create_complete_session_layout(&session);
    let child = session.join("context").join("child").join("work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "done\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(&child.join("result.md"), "Done.\n");
    write_text_file(&child.join("refs.jsonl"), "");

    let rows = agent_child_rows(&root, "coder", Some("default"));
    assert!(matches!(
        rows,
        Ok(ref rows) if rows.contains(&AgentChildRow {
            child: "work-123".to_owned(),
            status: "done".to_owned(),
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

#[test]
fn agent_child_rows_rejects_invalid_child_agent_metadata() {
    let root = clean_test_dir("ctx-child-row-invalid-child-agent");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let session = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );
    create_complete_session_layout(&session);
    let child = session.join("context").join("child").join("work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "../worker\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "done\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(&child.join("result.md"), "Done.\n");
    write_text_file(&child.join("refs.jsonl"), "");

    assert!(matches!(
        agent_child_rows(&root, "coder", Some("default")),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid child context: invalid agent name"
    ));
}

#[test]
fn agent_child_rows_rejects_mismatched_backing_parent() {
    let root = clean_test_dir("ctx-child-row-bad-parent");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let session = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );
    create_complete_session_layout(&session);
    let child = session.join("context").join("child").join("work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "done\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(&child.join("result.md"), "Done.\n");
    write_text_file(&child.join("refs.jsonl"), "");
    write_text_file(&root.join("agent/worker.d/parent"), "agent:planner\n");

    assert!(matches!(
        agent_child_rows(&root, "coder", Some("default")),
        Err(ref error)
            if error.code == 2
                && error.message
                    == "child work-123 backing parent mismatch for worker: agent:planner"
    ));
}

#[test]
fn agent_wait_rejects_invalid_terminal_child_session_metadata() {
    let root = clean_test_dir("ctx-agent-wait-invalid-child-session");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let session = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );
    create_complete_session_layout(&session);
    let child = session.join("context").join("child").join("work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "../default\n");
    write_text_file(&child.join("status"), "done\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(&child.join("result.md"), "Done.\n");
    write_text_file(&child.join("refs.jsonl"), "");

    assert!(matches!(
        agent_wait(&root, "coder", Some("default"), "work-123"),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid child context: invalid session name"
    ));
}

#[test]
fn agent_wait_rejects_mismatched_terminal_backing_parent() {
    let root = clean_test_dir("ctx-agent-wait-bad-parent");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let session = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );
    create_complete_session_layout(&session);
    let child = session.join("context").join("child").join("work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "done\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(&child.join("result.md"), "Done.\n");
    write_text_file(&child.join("refs.jsonl"), "");
    write_text_file(
        &root.join("agent/worker.d/parent"),
        "agent:coder session:other run:r1\n",
    );

    assert!(matches!(
        agent_wait(&root, "coder", Some("default"), "work-123"),
        Err(ref error)
            if error.code == 2
                && error.message
                    == "child work-123 backing parent mismatch for worker: agent:coder session:other run:r1"
    ));
}

#[test]
fn agent_wait_rejects_invalid_backing_lifecycle() {
    let root = clean_test_dir("ctx-agent-wait-invalid-backing-life");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let session = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );
    create_complete_session_layout(&session);
    let child = session.join("context").join("child").join("work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "done\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(&child.join("result.md"), "Done.\n");
    write_text_file(&child.join("refs.jsonl"), "");
    write_text_file(
        &root.join("agent/worker.d/parent"),
        "agent:coder session:default run:r1\n",
    );
    write_text_file(&root.join("agent/worker.d/life"), "detached\n");

    assert!(matches!(
        agent_wait(&root, "coder", Some("default"), "work-123"),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid agent life for worker: detached"
    ));
}

#[test]
fn agent_wait_rejects_invalid_backing_model() {
    let root = clean_test_dir("ctx-agent-wait-invalid-backing-model");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let session = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );
    create_complete_session_layout(&session);
    let child = session.join("context").join("child").join("work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "done\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(&child.join("result.md"), "Done.\n");
    write_text_file(&child.join("refs.jsonl"), "");
    write_text_file(
        &root.join("agent/worker.d/parent"),
        "agent:coder session:default run:r1\n",
    );
    write_text_file(&root.join("agent/worker.d/model"), "bad/model/name\n");

    assert!(matches!(
        agent_wait(&root, "coder", Some("default"), "work-123"),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid agent model for worker: bad/model/name"
    ));
}

#[test]
fn agent_wait_reaps_active_child_when_parent_session_is_omitted() {
    let root = clean_test_dir("ctx-wait-reaps-dead-no-session");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let session = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );
    create_complete_session_layout(&session);
    let child = session.join("context").join("child").join("work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "active\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(&child.join("result.md"), "");
    write_text_file(&child.join("refs.jsonl"), "");
    write_text_file(&root.join("agent/worker.d/parent"), "agent:coder run:r1\n");
    write_text_file(&root.join("agent/worker.d/status"), "dead\n");
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");
    write_text_file(&root.join("agent/worker.d/pid"), "\n");

    assert_eq!(
        agent_wait(&root, "coder", Some("default"), "work-123"),
        Ok(ExitCode::from(130))
    );
    assert!(matches!(
        fs::read_to_string(child.join("status")).as_deref(),
        Ok("cancelled\n")
    ));
    assert!(matches!(
        fs::read_to_string(child.join("result.md")).as_deref(),
        Ok("Child agent `worker` session `default` is dead.\n")
    ));
}

#[test]
fn agent_wait_reaps_active_child_when_backing_worker_pid_is_stale() {
    let root = clean_test_dir("ctx-agent-wait-reaps-stale-worker");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let session = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );
    create_complete_session_layout(&session);
    let child = session.join("context").join("child").join("work-stale");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "active\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(&child.join("result.md"), "");
    write_text_file(&child.join("refs.jsonl"), "");
    write_text_file(
        &root.join("agent/worker.d/parent"),
        "agent:coder session:default run:r1\n",
    );
    write_text_file(&root.join("agent/worker.d/status"), "busy\n");
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");
    write_text_file(&root.join("agent/worker.d/pid"), "999999999\n");

    let rows = agent_child_rows(&root, "coder", Some("default"));
    assert!(matches!(
        rows,
        Ok(ref rows) if rows.contains(&AgentChildRow {
            child: "work-stale".to_owned(),
            status: "cancelled".to_owned(),
            agent: "worker".to_owned(),
            session: "default".to_owned(),
            parent_session: Some("default".to_owned()),
            parent_run: Some("r1".to_owned()),
            model: "api.lmm.best/gpt-5.3-codex-spark".to_owned(),
            life: "temp".to_owned(),
            agent_status: "dead".to_owned(),
            ppid: None,
            pid: None,
        })
    ));
    assert_eq!(
        agent_wait(&root, "coder", Some("default"), "work-stale"),
        Ok(ExitCode::from(130))
    );
    assert!(matches!(
        fs::read_to_string(child.join("result.md")).as_deref(),
        Ok("Child agent `worker` session `default` is dead.\n")
    ));
}
