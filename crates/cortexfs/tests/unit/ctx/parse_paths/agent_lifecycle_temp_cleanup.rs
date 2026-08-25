#[test]
fn agent_wait_does_not_reap_canonical_temp_worker() -> Result<(), CliError> {
    let root = clean_test_dir("ctx-agent-wait-temp-worker-cleanup");
    create_agent_fixture(&root, "executor", "agent:base", "busy", "100");
    create_agent_fixture(&root, "worker", "agent:executor session:default run:r1", "busy", "101");
    write_text_file(&root.join("agent/executor.d/log"), "");
    write_text_file(&root.join("agent/worker.d/log"), "");
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");
    let socket = root.join("agent/worker.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket)
        .map_err(|error| CliError::unavailable(format!("cannot bind socket: {error}")))?;
    let session = ctx_home(&root)
        .unwrap_or_default()
        .join("agent/executor/session/default");
    create_complete_session_layout(&session);
    let child = session.join("context/child/work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "cancelled\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(
        &child.join("result.md"),
        "Child agent `worker` cancelled because the parent agent stopped.\n",
    );
    write_text_file(&child.join("refs.jsonl"), "");

    assert_eq!(
        agent_wait(&root, "executor", Some("default"), "work-123"),
        Ok(ExitCode::from(130))
    );

    assert!(root.join("agent/worker").exists());
    assert!(root.join("agent/worker.sock").exists());
    assert!(root.join("agent/worker.d").exists());
    assert_eq!(
        fs::read_to_string(child.join("result.md")).unwrap_or_default(),
        "Child agent `worker` cancelled because the parent agent stopped.\n"
    );
    assert_eq!(
        fs::read_to_string(child.join("status")).unwrap_or_default(),
        "cancelled\n"
    );
    Ok(())
}

#[test]
fn agent_wait_reaps_dedicated_temp_worker() -> Result<(), CliError> {
    let root = clean_test_dir("ctx-wait-temp-prefix");
    create_agent_fixture(&root, "executor", "agent:base", "busy", "100");
    create_agent_fixture(
        &root,
        "worker-fast",
        "agent:executor session:default run:r1",
        "busy",
        "101",
    );
    write_text_file(&root.join("agent/executor.d/log"), "");
    write_text_file(&root.join("agent/worker-fast.d/log"), "");
    write_text_file(&root.join("agent/worker-fast.d/life"), "temp\n");
    let socket = root.join("agent/worker-fast.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket)
        .map_err(|error| CliError::unavailable(format!("cannot bind socket: {error}")))?;
    let session = ctx_home(&root)
        .unwrap_or_default()
        .join("agent/executor/session/default");
    create_complete_session_layout(&session);
    let child = session.join("context/child/work-fast");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker-fast\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "cancelled\n");
    write_text_file(&child.join("handoff.md"), "Task: implement fast.\n");
    write_text_file(
        &child.join("result.md"),
        "Child agent `worker-fast` cancelled because the parent agent stopped.\n",
    );
    write_text_file(&child.join("refs.jsonl"), "");

    assert_eq!(
        agent_wait(&root, "executor", Some("default"), "work-fast"),
        Ok(ExitCode::from(130))
    );

    assert!(!root.join("agent/worker-fast").exists());
    assert!(!root.join("agent/worker-fast.sock").exists());
    assert!(!root.join("agent/worker-fast.d").exists());
    assert_eq!(
        fs::read_to_string(child.join("status")).unwrap_or_default(),
        "cancelled\n"
    );
    assert_eq!(
        fs::read_to_string(child.join("result.md")).unwrap_or_default(),
        "Child agent `worker-fast` cancelled because the parent agent stopped.\n"
    );
    Ok(())
}

#[test]
fn agent_stop_sends_stop_request_over_agent_socket() -> Result<(), CliError> {
    let root = clean_test_dir("ctx-agent-stop-request");
    create_agent_fixture(&root, "worker-fast", "agent:executor", "busy", "101");
    write_text_file(&root.join("agent/worker-fast.d/log"), "");
    write_text_file(&root.join("agent/worker-fast.d/life"), "temp\n");
    let socket = root.join("agent/worker-fast.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket)
        .map_err(|error| CliError::unavailable(format!("cannot bind socket: {error}")))?;
    let server = std::thread::spawn(move || -> Result<(), String> {
        let (mut stream, _address) = listener.accept().map_err(|error| error.to_string())?;
        let mut request = String::new();
        std::io::Read::read_to_string(&mut stream, &mut request)
            .map_err(|error| error.to_string())?;
        let request: serde_json::Value =
            serde_json::from_str(&request).map_err(|error| error.to_string())?;
        assert_eq!(
            request,
            serde_json::json!({ "op": "stop", "agent": "worker-fast" })
        );
        std::io::Write::write_all(&mut stream, b"{\"type\":\"done\"}\n")
            .map_err(|error| error.to_string())?;
        Ok(())
    });

    assert_eq!(agent_stop(&root, "worker-fast"), Ok(ExitCode::SUCCESS));
    assert!(matches!(server.join(), Ok(Ok(()))));
    Ok(())
}

#[test]
fn temp_cleanup_preflights_before_removal() {
    let root = clean_test_dir("ctx-stop-temp-cleanup-preflight");
    create_agent_fixture(
        &root,
        "worker-fast",
        "agent:executor session:default run:r1",
        "busy",
        "101",
    );
    write_text_file(&root.join("agent/worker-fast.d/log"), "");
    write_text_file(&root.join("agent/worker-fast.d/life"), "temp\n");
    assert!(fs::remove_file(root.join("agent/worker-fast")).is_ok());
    assert!(fs::create_dir_all(root.join("agent/worker-fast")).is_ok());
    let result = remove_temp_agent_object(&root, "worker-fast");

    assert!(matches!(result, Err(ref error) if error.code == 69));
    assert!(root.join("agent/worker-fast").is_dir());
    assert_eq!(
        fs::read_to_string(root.join("agent/worker-fast.d/status")).unwrap_or_default(),
        "busy\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/worker-fast.d/pid")).unwrap_or_default(),
        "101\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/worker-fast.d/log")).unwrap_or_default(),
        ""
    );
}

#[test]
fn temp_cleanup_rejects_unwritable_control_subtree_before_removal() {
    if nix::unistd::Uid::effective().is_root() {
        return;
    }
    let root = clean_test_dir("ctx-stop-temp-control-preflight");
    create_agent_fixture(&root, "worker-fast", "agent:executor", "busy", "101");
    write_text_file(&root.join("agent/worker-fast.d/log"), "");
    write_text_file(&root.join("agent/worker-fast.d/life"), "temp\n");
    let cache = root.join("agent/worker-fast.d/cache");
    assert!(fs::create_dir_all(&cache).is_ok());
    assert!(fs::set_permissions(&cache, fs::Permissions::from_mode(0o555)).is_ok());

    let result = remove_temp_agent_object(&root, "worker-fast");
    assert!(fs::set_permissions(&cache, fs::Permissions::from_mode(0o755)).is_ok());

    assert!(matches!(result, Err(ref error) if error.code == 69));
    assert!(root.join("agent/worker-fast").is_file());
    assert_eq!(
        fs::read_to_string(root.join("agent/worker-fast.d/status")).unwrap_or_default(),
        "busy\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/worker-fast.d/log")).unwrap_or_default(),
        ""
    );
}

#[test]
fn temp_cleanup_unlinks_control_symlink_without_following_target() {
    let root = clean_test_dir("ctx-stop-temp-control-symlink");
    let outside = clean_test_dir("ctx-stop-temp-control-symlink-outside");
    create_agent_fixture(&root, "worker-fast", "agent:executor", "busy", "101");
    write_text_file(&root.join("agent/worker-fast.d/log"), "");
    write_text_file(&root.join("agent/worker-fast.d/life"), "temp\n");
    write_text_file(&outside.join("keep"), "keep\n");
    assert!(
        symlink(
            &*outside,
            root.join("agent/worker-fast.d/outside")
        )
        .is_ok()
    );

    assert_eq!(remove_temp_agent_object(&root, "worker-fast"), Ok(()));

    assert_eq!(
        fs::read_to_string(outside.join("keep")).unwrap_or_default(),
        "keep\n"
    );
    assert!(!root.join("agent/worker-fast.d").exists());
}
