#[test]
fn parses_agent_lifecycle_commands() {
    let new = cmd!(
        "agent",
        "new",
        "reviewer",
        "--temp",
        "--label",
        "reviewer_t",
        "--model",
        "openai/gpt-4o",
        "--tool",
        "fs.read",
        "--shared",
        "project-a:read",
        "--mount",
        "/work",
        "/work",
        "ro",
    );
    assert!(matches!(
        new,
        Ok(Command::Agent(AgentArgs::New(ref args)))
            if args.name == "reviewer"
                && args.temporary
                && args.label.as_deref() == Some("reviewer_t")
                && args.models == ["openai/gpt-4o".to_owned()]
                && args.tools == ["fs.read".to_owned()]
                && args.shared.len() == 1
                && args.mounts.len() == 1
    ));

    let start = cmd!(
        "agent",
        "start",
        "reviewer",
        "--session",
        "test",
        "--mount",
        "/repo",
        "/workspace",
        "rw",
        "--cwd",
        "/workspace",
    );
    assert!(matches!(
        start,
        Ok(Command::Agent(AgentArgs::Start(ref args)))
            if args.name == "reviewer"
                && args.session == "test"
                && args.cwd == "/workspace"
                && args.default_workspace
                && args.mounts.len() == 1
    ));

    let stop = cmd!("agent", "stop", "reviewer");
    assert!(matches!(
        stop,
        Ok(Command::Agent(AgentArgs::Stop { ref name })) if name == "reviewer"
    ));

    let status = cmd!("agent", "status", "reviewer");
    assert!(matches!(
        status,
        Ok(Command::Agent(AgentArgs::Status { ref name })) if name == "reviewer"
    ));

    let ps = cmd!("agent", "ps");
    assert!(matches!(ps, Ok(Command::Agent(AgentArgs::Ps))));

    let watch = cmd!("agent", "watch", "coder", "--session", "test");
    assert!(matches!(
        watch,
        Ok(Command::Agent(AgentArgs::Watch {
            ref name,
            session: Some(ref session)
        })) if name == "coder" && session == "test"
    ));

    let attach = cmd!("agent", "attach", "coder");
    assert!(matches!(
        attach,
        Ok(Command::Agent(AgentArgs::Attach {
            ref name,
            session: None
        })) if name == "coder"
    ));
}

#[test]
fn agent_lifecycle_tool_command_uses_clean_runtime_environment() {
    let root = Path::new("/tmp/cortexfs-clean-lifecycle-root");
    let command = agent_lifecycle_tool_command(root, &root.join("tool").join("agent.create"));
    let mut envs = command
        .get_envs()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect::<Vec<_>>();
    envs.sort();

    assert_eq!(
        envs,
        vec![
            (
                "CTX_ROOT".to_owned(),
                Some(root.display().to_string())
            ),
            ("PATH".to_owned(), Some("/usr/bin:/bin".to_owned())),
        ]
    );
}

#[test]
fn agent_new_request_json_matches_lifecycle_tool_shape() {
    let command = cmd!(
        "agent",
        "new",
        "reviewer",
        "--label",
        "reviewer_t",
        "--model",
        "openai/gpt-4o",
        "--tool",
        "fs.read",
        "--shared",
        "project-a:read",
        "--mount",
        "/work",
        "/work",
        "ro",
    );
    assert!(matches!(command, Ok(Command::Agent(AgentArgs::New(_)))));
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };
    assert_eq!(
        agent_new_request_json(&args),
        Ok(
            "{\"name\":\"reviewer\",\"label\":\"reviewer_t\",\"model\":[\"openai/gpt-4o\"],\"tools\":[\"fs.read\"],\"shared\":{\"project-a\":[\"read\"]},\"mount\":[[\"/work\",\"/work\",\"ro\"]]}".to_owned()
        )
    );
}

#[test]
fn agent_new_temp_request_json_includes_lifecycle() {
    let command = cmd!("agent", "new", "scratch", "--temp");
    assert!(matches!(command, Ok(Command::Agent(AgentArgs::New(_)))));
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };
    assert_eq!(
        agent_new_request_json(&args),
        Ok("{\"name\":\"scratch\",\"life\":\"temp\"}".to_owned())
    );
}

#[test]
fn agent_new_request_json_accepts_explicit_parent_ref() {
    let command = cmd!(
        "agent",
        "new",
        "work-123",
        "--temp",
        "--parent",
        "agent:coder session:default run:r1",
    );
    assert!(matches!(command, Ok(Command::Agent(AgentArgs::New(_)))));
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };
    assert_eq!(
        agent_new_request_json(&args),
        Ok("{\"name\":\"work-123\",\"life\":\"temp\",\"parent\":\"agent:coder session:default run:r1\"}".to_owned())
    );
}

#[test]
fn agent_new_host_fallback_creates_spark_worker_when_lifecycle_tool_is_absent() {
    let root = clean_test_dir("ctx-agent-new-host-worker-fallback");
    let command = cmd!(
        "agent",
        "new",
        "worker",
        "--temp",
        "--parent",
        "agent:coder",
        "--label",
        "worker_t",
        "--model",
        "api.lmm.best/gpt-5.3-codex-spark",
        "--tool",
        "tsh.config",
    );
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };

    assert_eq!(agent_new(&root, &args), Ok(ExitCode::SUCCESS));
    assert!(root.join("agent/worker").is_file());
    assert_eq!(
        fs::read_to_string(root.join("agent/worker.d/label"))
            .unwrap_or_default(),
        "user_u:agent_r:worker_t:s0\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/worker.d/life"))
            .unwrap_or_default(),
        "temp\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/worker.d/parent")).unwrap_or_default(),
        "agent:coder\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/worker.d/model"))
            .unwrap_or_default(),
        "api.lmm.best/gpt-5.3-codex-spark\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/worker.d/policy"))
            .unwrap_or_default(),
        "allow worker_t model:api.lmm.best/gpt-5.3-codex-spark use\nallow worker_t tool:tsh execute\nallow worker_t network:default connect\nallow worker_t tool:tsh.config execute\n"
    );
    let home = ctx_home(&root).unwrap_or_default();
    assert!(home.join("agent/worker/root").is_dir());
    assert!(
        home.join("agent/worker/session/index/by-cwd")
            .is_dir()
    );

    let duplicate = agent_new(&root, &args);
    assert!(matches!(
        duplicate,
        Err(ref error)
            if error.code == 69 && error.message == "agent already exists: worker"
    ));
}

#[test]
fn agent_stop_host_fallback_marks_agent_dead_and_records_stop_event() {
    let root = clean_test_dir("ctx-agent-stop-host-fallback");
    create_agent_fixture(&root, "scratch", "agent:base", "busy", "4242");
    write_text_file(&root.join("agent/scratch.d/log"), "");

    assert_eq!(agent_stop(&root, "scratch"), Ok(ExitCode::SUCCESS));
    assert_eq!(
        fs::read_to_string(root.join("agent/scratch.d/status")).unwrap_or_default(),
        "dead\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/scratch.d/pid")).unwrap_or_default(),
        "\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/scratch.d/log")).unwrap_or_default(),
        "{\"type\":\"agent.stop\",\"agent\":\"scratch\",\"status\":\"cancelled\"}\n"
    );
}

#[test]
fn agent_terminal_units_follow_existing_session_names() {
    let root = clean_test_dir("ctx-agent-terminal-units");
    let session_root = ctx_home(&root)
        .unwrap_or_default()
        .join("agent/coder/session");
    assert!(fs::create_dir_all(session_root.join("default")).is_ok());
    assert!(fs::create_dir_all(session_root.join("feature-a")).is_ok());
    assert!(fs::create_dir_all(session_root.join("bad.name")).is_ok());

    assert_eq!(
        agent_terminal_units(&root, "coder"),
        Ok(vec![
            "cortexfs-agent-coder-bad-name-terminal".to_owned(),
            "cortexfs-agent-coder-default-terminal".to_owned(),
            "cortexfs-agent-coder-feature-a-terminal".to_owned(),
        ])
    );
}

#[test]
fn agent_stop_host_fallback_cancels_owned_child_agents() {
    let root = clean_test_dir("ctx-agent-stop-host-fallback-children");
    create_agent_fixture(&root, "coder", "agent:base", "busy", "100");
    create_agent_fixture(
        &root,
        "worker",
        "agent:coder session:default run:r1",
        "busy",
        "101",
    );
    create_agent_fixture(&root, "nested", "agent:worker", "ready", "102");
    create_agent_fixture(&root, "reviewer", "agent:base", "busy", "103");
    write_text_file(&root.join("agent/coder.d/log"), "");
    write_text_file(&root.join("agent/worker.d/log"), "");
    write_text_file(&root.join("agent/nested.d/log"), "");
    write_text_file(&root.join("agent/reviewer.d/log"), "");
    assert!(fs::create_dir_all(
        ctx_home(&root)
            .unwrap_or_default()
            .join("agent/worker/session/work-live")
    )
    .is_ok());
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

    for name in ["coder", "worker", "nested"] {
        assert_eq!(
            fs::read_to_string(root.join(format!("agent/{name}.d/status"))).unwrap_or_default(),
            "dead\n",
            "{name}"
        );
        assert_eq!(
            fs::read_to_string(root.join(format!("agent/{name}.d/pid"))).unwrap_or_default(),
            "\n",
            "{name}"
        );
        assert_eq!(
            fs::read_to_string(root.join(format!("agent/{name}.d/log"))).unwrap_or_default(),
            format!(r#"{{"type":"agent.stop","agent":"{name}","status":"cancelled"}}"#) + "\n",
            "{name}"
        );
    }
    assert_eq!(
        fs::read_to_string(root.join("agent/reviewer.d/status")).unwrap_or_default(),
        "busy\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/reviewer.d/pid")).unwrap_or_default(),
        "103\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/reviewer.d/log")).unwrap_or_default(),
        ""
    );
    assert_eq!(
        fs::read_to_string(child.join("status")).unwrap_or_default(),
        "cancelled\n"
    );
    assert_eq!(
        fs::read_to_string(child.join("result.md")).unwrap_or_default(),
        "Child agent `worker` cancelled because the parent agent stopped.\n"
    );
    assert_eq!(
        agent_terminal_units(&root, "worker"),
        Ok(vec!["cortexfs-agent-worker-work-live-terminal".to_owned()])
    );
    assert_eq!(
        agent_wait(&root, "coder", Some("default"), "work-123"),
        Ok(ExitCode::from(130))
    );
}

#[test]
fn agent_stop_host_fallback_cancels_sessionless_owned_child_agent() {
    let root = clean_test_dir("ctx-agent-stop-host-fallback-sessionless-child");
    create_agent_fixture(&root, "coder", "agent:base", "busy", "100");
    create_agent_fixture(&root, "worker", "agent:coder", "busy", "101");
    write_text_file(&root.join("agent/coder.d/log"), "");
    write_text_file(&root.join("agent/worker.d/log"), "");
    let session = ctx_home(&root)
        .unwrap_or_default()
        .join("agent/coder/session/feature");
    create_complete_session_layout(&session);
    let child = session.join("context/child/work-live");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "feature\n");
    write_text_file(&child.join("status"), "active\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(&child.join("result.md"), "");
    write_text_file(&child.join("refs.jsonl"), "");

    assert_eq!(agent_stop(&root, "coder"), Ok(ExitCode::SUCCESS));

    assert_eq!(
        fs::read_to_string(root.join("agent/worker.d/status")).unwrap_or_default(),
        "dead\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/worker.d/pid")).unwrap_or_default(),
        "\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/worker.d/log")).unwrap_or_default(),
        "{\"type\":\"agent.stop\",\"agent\":\"worker\",\"status\":\"cancelled\"}\n"
    );
    assert_eq!(
        fs::read_to_string(child.join("status")).unwrap_or_default(),
        "cancelled\n"
    );
    assert_eq!(
        agent_wait(&root, "coder", Some("feature"), "work-live"),
        Ok(ExitCode::from(130))
    );
}

#[test]
fn agent_stop_host_fallback_rejects_symlink_control_file() {
    let root = clean_test_dir("ctx-agent-stop-host-fallback-symlink");
    let outside = clean_test_dir("ctx-agent-stop-host-fallback-symlink-outside");
    create_agent_fixture(&root, "scratch", "agent:base", "busy", "4242");
    assert!(fs::remove_file(root.join("agent/scratch.d/status")).is_ok());
    assert!(std::os::unix::fs::symlink(
        outside.join("status-target"),
        root.join("agent/scratch.d/status")
    )
    .is_ok());

    let result = agent_stop(&root, "scratch");
    assert!(matches!(
        result,
        Err(ref error)
            if error.code == 69
                && error.message.contains("refusing symlink control file")
    ));
    assert!(!outside.join("status-target").exists());
}
