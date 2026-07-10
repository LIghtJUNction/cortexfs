#[test]
fn agent_stop_host_fallback_retains_canonical_temp_worker_without_cancellation(
) -> Result<(), CliError> {
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

    assert!(root.join("agent/worker").exists());
    assert!(root.join("agent/worker.sock").exists());
    assert!(root.join("agent/worker.d").exists());
    assert_eq!(
        fs::read_to_string(root.join("agent/worker.d/status")).unwrap_or_default(),
        "busy\n"
    );
    assert_eq!(
        fs::read_to_string(child.join("result.md")).unwrap_or_default(),
        ""
    );
    assert_eq!(
        fs::read_to_string(child.join("status")).unwrap_or_default(),
        "active\n"
    );
    Ok(())
}

#[test]
fn agent_stop_host_fallback_removes_temp_worker_prefix_object() -> Result<(), CliError> {
    let root = clean_test_dir("ctx-stop-temp-prefix");
    create_agent_fixture(&root, "coder", "agent:base", "busy", "100");
    create_agent_fixture(
        &root,
        "worker-fast",
        "agent:coder session:default run:r1",
        "busy",
        "101",
    );
    write_text_file(&root.join("agent/coder.d/log"), "");
    write_text_file(&root.join("agent/worker-fast.d/log"), "");
    write_text_file(&root.join("agent/worker-fast.d/life"), "temp\n");
    let socket = root.join("agent/worker-fast.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket)
        .map_err(|error| CliError::unavailable(format!("cannot bind socket: {error}")))?;
    let session = ctx_home(&root)
        .unwrap_or_default()
        .join("agent/coder/session/default");
    create_complete_session_layout(&session);
    let child = session.join("context/child/work-fast");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker-fast\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "active\n");
    write_text_file(&child.join("handoff.md"), "Task: implement fast.\n");
    write_text_file(&child.join("result.md"), "");
    write_text_file(&child.join("refs.jsonl"), "");

    assert_eq!(agent_stop(&root, "coder"), Ok(ExitCode::SUCCESS));

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
    assert_eq!(
        agent_wait(&root, "coder", Some("default"), "work-fast"),
        Ok(ExitCode::from(130))
    );
    Ok(())
}

#[test]
fn agent_stop_host_fallback_preflights_temp_cleanup_before_any_stop_write() {
    let root = clean_test_dir("ctx-stop-temp-cleanup-preflight");
    create_agent_fixture(&root, "coder", "agent:base", "busy", "100");
    create_agent_fixture(
        &root,
        "worker-fast",
        "agent:coder session:default run:r1",
        "busy",
        "101",
    );
    write_text_file(&root.join("agent/coder.d/log"), "");
    write_text_file(&root.join("agent/worker-fast.d/log"), "");
    write_text_file(&root.join("agent/worker-fast.d/life"), "temp\n");
    assert!(fs::remove_file(root.join("agent/worker-fast")).is_ok());
    assert!(fs::create_dir_all(root.join("agent/worker-fast")).is_ok());
    let session = ctx_home(&root)
        .unwrap_or_default()
        .join("agent/coder/session/default");
    create_complete_session_layout(&session);
    let child = session.join("context/child/work-fast");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker-fast\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "active\n");
    write_text_file(&child.join("handoff.md"), "Task: cleanup preflight.\n");
    write_text_file(&child.join("result.md"), "");
    write_text_file(&child.join("refs.jsonl"), "");

    let result = agent_stop(&root, "coder");

    assert!(matches!(result, Err(ref error) if error.code == 69));
    assert_eq!(
        fs::read_to_string(root.join("agent/coder.d/status")).unwrap_or_default(),
        "busy\n"
    );
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
    assert_eq!(
        fs::read_to_string(child.join("status")).unwrap_or_default(),
        "active\n"
    );
    assert_eq!(
        fs::read_to_string(child.join("result.md")).unwrap_or_default(),
        ""
    );
}

#[test]
fn agent_stop_host_fallback_rejects_unwritable_temp_control_subtree_before_writes() {
    if nix::unistd::Uid::effective().is_root() {
        return;
    }
    let root = clean_test_dir("ctx-stop-temp-control-preflight");
    create_agent_fixture(&root, "coder", "agent:base", "busy", "100");
    create_agent_fixture(&root, "worker-fast", "agent:coder", "busy", "101");
    write_text_file(&root.join("agent/coder.d/log"), "");
    write_text_file(&root.join("agent/worker-fast.d/log"), "");
    write_text_file(&root.join("agent/worker-fast.d/life"), "temp\n");
    let cache = root.join("agent/worker-fast.d/cache");
    assert!(fs::create_dir_all(&cache).is_ok());
    assert!(fs::set_permissions(&cache, fs::Permissions::from_mode(0o555)).is_ok());

    let result = agent_stop(&root, "coder");
    assert!(fs::set_permissions(&cache, fs::Permissions::from_mode(0o755)).is_ok());

    assert!(matches!(result, Err(ref error) if error.code == 69));
    assert_eq!(
        fs::read_to_string(root.join("agent/coder.d/status")).unwrap_or_default(),
        "busy\n"
    );
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
fn agent_stop_host_fallback_unlinks_temp_control_symlink_without_following_target() {
    let root = clean_test_dir("ctx-stop-temp-control-symlink");
    let outside = clean_test_dir("ctx-stop-temp-control-symlink-outside");
    create_agent_fixture(&root, "coder", "agent:base", "busy", "100");
    create_agent_fixture(&root, "worker-fast", "agent:coder", "busy", "101");
    write_text_file(&root.join("agent/coder.d/log"), "");
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

    assert_eq!(agent_stop(&root, "coder"), Ok(ExitCode::SUCCESS));

    assert_eq!(
        fs::read_to_string(outside.join("keep")).unwrap_or_default(),
        "keep\n"
    );
    assert!(!root.join("agent/worker-fast.d").exists());
}
