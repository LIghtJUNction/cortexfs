use std::os::unix::fs::FileTypeExt;

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
    assert!(matches!(
        fs::symlink_metadata(root.join("agent/worker.sock")),
        Ok(metadata) if metadata.file_type().is_socket()
    ));
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
fn agent_new_host_fallback_rejects_existing_socket_or_symlink() {
    for kind in ["socket", "symlink"] {
        let root = clean_test_dir(&format!("ctx-agent-new-host-{kind}-conflict"));
        assert!(fs::create_dir_all(root.join("agent")).is_ok());
        let socket = root.join("agent/worker.sock");
        let _listener = if kind == "socket" {
            std::os::unix::net::UnixListener::bind(&socket).ok()
        } else {
            assert!(std::os::unix::fs::symlink("missing-runtime.sock", &socket).is_ok());
            None
        };
        let Ok(Command::Agent(AgentArgs::New(args))) = cmd!("agent", "new", "worker") else {
            return;
        };

        assert!(agent_new_host_fallback(&root, &args).is_err(), "{kind}");
        assert!(fs::symlink_metadata(&socket).is_ok(), "{kind}");
        assert!(!root.join("agent/worker").exists(), "{kind}");
        assert!(!root.join("agent/worker.d").exists(), "{kind}");
    }
}

#[test]
fn agent_new_host_fallback_rejects_existing_home_without_partial_object() {
    let root = clean_test_dir("ctx-agent-new-host-home-conflict");
    let home = ctx_home(&root)
        .unwrap_or_default()
        .join("agent/worker");
    assert!(fs::create_dir_all(&home).is_ok());
    assert!(fs::write(home.join("keep"), "keep\n").is_ok());
    let Ok(Command::Agent(AgentArgs::New(args))) = cmd!("agent", "new", "worker") else {
        return;
    };

    assert!(agent_new_host_fallback(&root, &args).is_err());
    assert_eq!(fs::read_to_string(home.join("keep")).unwrap_or_default(), "keep\n");
    assert!(!root.join("agent/worker").exists());
    assert!(!root.join("agent/worker.d").exists());
    assert!(!root.join("agent/worker.sock").exists());
}

#[test]
fn agent_new_host_fallback_rolls_back_after_socket_creation_failure() {
    let base = clean_test_dir("ctx-agent-new-host-socket-rollback");
    let root = base.join("x".repeat(80));
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(root.join("agent/worker.sock").as_os_str().len() > 108);
    let Ok(Command::Agent(AgentArgs::New(args))) = cmd!("agent", "new", "worker") else {
        return;
    };

    assert!(agent_new_host_fallback(&root, &args).is_err());
    assert!(!root.join("agent/worker").exists());
    assert!(!root.join("agent/worker.d").exists());
    assert!(fs::symlink_metadata(root.join("agent/worker.sock")).is_err());
    assert!(!ctx_home(&root).unwrap_or_default().join("agent/worker").exists());
}

#[test]
fn agent_new_host_fallback_rolls_back_after_home_creation_failure() {
    if nix::unistd::Uid::current().is_root() {
        return;
    }
    let root = clean_test_dir("ctx-agent-new-host-home-rollback");
    let home_parent = ctx_home(&root)
        .unwrap_or_default()
        .join("agent");
    assert!(fs::create_dir_all(&home_parent).is_ok());
    assert!(fs::set_permissions(&home_parent, fs::Permissions::from_mode(0o555)).is_ok());
    let Ok(Command::Agent(AgentArgs::New(args))) = cmd!("agent", "new", "worker") else {
        return;
    };

    assert!(agent_new_host_fallback(&root, &args).is_err());
    assert!(!root.join("agent/worker").exists());
    assert!(!root.join("agent/worker.d").exists());
    assert!(fs::symlink_metadata(root.join("agent/worker.sock")).is_err());
    assert!(!home_parent.join("worker").exists());
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
fn agent_control_updates_are_atomic_and_preserve_mode_while_log_appends() {
    let root = clean_test_dir("agent-control-atomic-update");
    let control = root.join("agent/scratch.d");
    assert!(fs::create_dir_all(&control).is_ok());
    let status = control.join("status");
    let pid = control.join("pid");
    let log = control.join("log");
    write_text_file(&status, "ready\n");
    write_text_file(&pid, "123\n");
    write_text_file(&log, "before\n");
    assert!(fs::set_permissions(&status, fs::Permissions::from_mode(0o644)).is_ok());
    assert!(fs::set_permissions(&pid, fs::Permissions::from_mode(0o640)).is_ok());
    let old_status_inode = fs::symlink_metadata(&status)
        .map(|metadata| metadata.ino())
        .unwrap_or_default();

    assert_eq!(write_agent_control_plain(&status, "dead\n"), Ok(()));
    assert_eq!(write_agent_control_plain(&pid, "\n"), Ok(()));
    assert_eq!(append_agent_log_event(&log, "after"), Ok(()));

    assert_eq!(fs::read_to_string(&status).unwrap_or_default(), "dead\n");
    assert_eq!(fs::read_to_string(&pid).unwrap_or_default(), "\n");
    assert_eq!(fs::read_to_string(&log).unwrap_or_default(), "before\nafter\n");
    assert!(matches!(
        fs::symlink_metadata(&status),
        Ok(ref metadata)
            if metadata.permissions().mode() & 0o7777 == 0o644
                && metadata.ino() != old_status_inode
    ));
    assert!(matches!(
        fs::symlink_metadata(&pid),
        Ok(ref metadata) if metadata.permissions().mode() & 0o7777 == 0o640
    ));
}

#[test]
fn agent_control_update_rejects_symlink_and_non_regular_target() {
    let root = clean_test_dir("agent-control-atomic-reject");
    let target = root.join("target");
    let link = root.join("link");
    let directory = root.join("directory");
    write_text_file(&target, "keep\n");
    assert!(symlink(&target, &link).is_ok());
    assert!(fs::create_dir_all(&directory).is_ok());

    assert!(write_agent_control_plain(&link, "bad\n").is_err());
    assert!(write_agent_control_plain(&directory, "bad\n").is_err());
    assert_eq!(fs::read_to_string(target).unwrap_or_default(), "keep\n");
}

#[test]
fn agent_control_update_refuses_readonly_target_without_changes() {
    if nix::unistd::Uid::effective().is_root() {
        return;
    }
    let root = clean_test_dir("agent-control-readonly");
    assert!(fs::create_dir_all(&root).is_ok());
    let status = root.join("status");
    write_text_file(&status, "ready\n");
    assert!(fs::set_permissions(&status, fs::Permissions::from_mode(0o444)).is_ok());
    let before = fs::symlink_metadata(&status)
        .map(|metadata| {
            (
                metadata.ino(),
                metadata.permissions().mode() & 0o7777,
                metadata.uid(),
                metadata.gid(),
            )
        })
        .ok();

    assert!(write_agent_control_plain(&status, "dead\n").is_err());
    assert_eq!(fs::read_to_string(&status).unwrap_or_default(), "ready\n");
    assert_eq!(
        fs::symlink_metadata(status)
            .map(|metadata| (
                metadata.ino(),
                metadata.permissions().mode() & 0o7777,
                metadata.uid(),
                metadata.gid(),
            ))
            .ok(),
        before
    );
}

#[test]
fn agent_session_update_preserves_existing_workspace_metadata() {
    let root = clean_test_dir("agent-session-workspace-preserve");
    assert!(fs::create_dir_all(&root).is_ok());
    let workspace = root.join("workspace");
    write_text_file(&workspace, "/old\n");
    assert!(fs::set_permissions(&workspace, fs::Permissions::from_mode(0o640)).is_ok());
    let before = fs::symlink_metadata(&workspace)
        .map(|metadata| {
            (
                metadata.ino(),
                metadata.permissions().mode() & 0o7777,
                metadata.uid(),
                metadata.gid(),
            )
        })
        .ok();

    assert_eq!(write_agent_session_plain(&workspace, "/new\n"), Ok(()));
    assert_eq!(fs::read_to_string(&workspace).unwrap_or_default(), "/new\n");
    assert!(matches!(
        (before, fs::symlink_metadata(workspace)),
        (Some((inode, mode, uid, gid)), Ok(metadata))
            if metadata.ino() != inode
                && metadata.permissions().mode() & 0o7777 == mode
                && metadata.uid() == uid
                && metadata.gid() == gid
    ));
}

#[test]
fn agent_session_missing_fallback_does_not_replace_symlink() {
    let root = clean_test_dir("agent-session-workspace-symlink");
    assert!(fs::create_dir_all(&root).is_ok());
    let target = root.join("target");
    let workspace = root.join("workspace");
    write_text_file(&target, "keep\n");
    assert!(symlink(&target, &workspace).is_ok());

    assert!(write_agent_session_plain(&workspace, "/bad\n").is_err());

    assert_eq!(fs::read_to_string(target).unwrap_or_default(), "keep\n");
    assert!(workspace
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink()));
}

#[test]
fn agent_session_update_atomically_creates_missing_workspace() {
    let root = clean_test_dir("agent-session-workspace-create");
    let workspace = root.join("workspace");
    assert!(fs::create_dir_all(&root).is_ok());

    assert_eq!(
        write_agent_session_plain(&workspace, "/workspace\n"),
        Ok(())
    );
    assert_eq!(
        fs::read_to_string(&workspace).unwrap_or_default(),
        "/workspace\n"
    );
    assert!(matches!(
        fs::symlink_metadata(workspace),
        Ok(metadata) if metadata.permissions().mode() & 0o7777 == 0o600
    ));
}

#[test]
fn agent_host_stub_replacement_is_atomic_and_ignores_umask() -> std::io::Result<()> {
    const CHILD_ENV: &str = "CORTEXFS_TEST_AGENT_HOST_STUB_UMASK_CHILD";
    if std::env::var_os(CHILD_ENV).is_some() {
        let root = clean_test_dir("agent-host-stub-atomic");
        fs::create_dir_all(&root)?;
        let wrapper = root.join("agent-wrapper");
        write_text_file(&wrapper, "old\n");
        write_agent_host_stub(&wrapper, "first")
            .map_err(|error| std::io::Error::other(error.message))?;
        let first_inode = fs::symlink_metadata(&wrapper)?.ino();

        write_agent_host_stub(&wrapper, "second")
            .map_err(|error| std::io::Error::other(error.message))?;

        assert_eq!(fs::read_to_string(&wrapper)?, agent_host_stub_script("second"));
        let metadata = fs::symlink_metadata(wrapper)?;
        assert_ne!(metadata.ino(), first_inode);
        assert_eq!(metadata.permissions().mode() & 0o7777, 0o755);
        return Ok(());
    }

    let test_binary = std::env::current_exe()?;
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg("umask 077; exec \"$1\" tests::agent_host_stub_replacement_is_atomic_and_ignores_umask --exact")
        .arg("sh")
        .arg(test_binary)
        .env(CHILD_ENV, "1")
        .status()?;
    assert!(status.success());
    Ok(())
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
        "helper",
        "agent:coder session:default run:r1",
        "busy",
        "101",
    );
    create_agent_fixture(&root, "nested", "agent:helper", "ready", "102");
    create_agent_fixture(&root, "reviewer", "agent:base", "busy", "103");
    write_text_file(&root.join("agent/coder.d/log"), "");
    write_text_file(&root.join("agent/helper.d/log"), "");
    write_text_file(&root.join("agent/nested.d/log"), "");
    write_text_file(&root.join("agent/reviewer.d/log"), "");
    assert!(fs::create_dir_all(
        ctx_home(&root)
            .unwrap_or_default()
            .join("agent/helper/session/work-live")
    )
    .is_ok());
    let session = ctx_home(&root)
        .unwrap_or_default()
        .join("agent/coder/session/default");
    create_complete_session_layout(&session);
    let child = session.join("context/child/work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "helper\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "active\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(&child.join("result.md"), "");
    write_text_file(&child.join("refs.jsonl"), "");

    assert_eq!(agent_stop(&root, "coder"), Ok(ExitCode::SUCCESS));

    for name in ["coder", "helper", "nested"] {
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
        "Child agent `helper` cancelled because the parent agent stopped.\n"
    );
    assert_eq!(
        agent_terminal_units(&root, "helper"),
        Ok(vec!["cortexfs-agent-helper-work-live-terminal".to_owned()])
    );
    assert_eq!(
        agent_wait(&root, "coder", Some("default"), "work-123"),
        Ok(ExitCode::from(130))
    );
}

#[test]
fn agent_stop_host_fallback_retains_unwritable_retired_worker() {
    let root = clean_test_dir("ctx-agent-stop-retired-worker");
    create_agent_fixture(&root, "coder", "agent:base", "busy", "100");
    create_agent_fixture(
        &root,
        "worker",
        "agent:coder session:default run:r1",
        "busy",
        "101",
    );
    write_text_file(&root.join("agent/coder.d/log"), "");
    write_text_file(&root.join("agent/worker.d/log"), "retained\n");
    let metadata = fs::metadata(root.join("agent/worker.d/status"));
    assert!(metadata.is_ok(), "{metadata:?}");
    let Ok(metadata) = metadata else {
        return;
    };
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o444);
    assert!(
        fs::set_permissions(root.join("agent/worker.d/status"), permissions).is_ok()
    );
    let session = ctx_home(&root)
        .unwrap_or_default()
        .join("agent/coder/session/default");
    create_complete_session_layout(&session);
    let child = session.join("context/child/work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "active\n");
    write_text_file(&child.join("handoff.md"), "Task: retained worker.\n");
    write_text_file(&child.join("result.md"), "");
    write_text_file(&child.join("refs.jsonl"), "");

    assert_eq!(agent_stop(&root, "coder"), Ok(ExitCode::SUCCESS));

    assert_eq!(
        fs::read_to_string(root.join("agent/coder.d/status")).unwrap_or_default(),
        "dead\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/worker.d/status")).unwrap_or_default(),
        "busy\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/worker.d/pid")).unwrap_or_default(),
        "101\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/worker.d/log")).unwrap_or_default(),
        "retained\n"
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
fn agent_stop_host_fallback_validates_owned_children_before_stopping_parent() {
    let root = clean_test_dir("ctx-agent-stop-invalid-owned-child");
    let outside = clean_test_dir("ctx-agent-stop-invalid-owned-child-outside");
    create_agent_fixture(&root, "coder", "agent:base", "busy", "100");
    create_agent_fixture(&root, "helper", "agent:coder", "busy", "101");
    write_text_file(&root.join("agent/coder.d/log"), "");
    assert!(fs::remove_file(root.join("agent/helper.d/life")).is_ok());
    assert!(
        symlink(
            outside.join("life"),
            root.join("agent/helper.d/life")
        )
        .is_ok()
    );

    let result = agent_stop(&root, "coder");

    assert!(matches!(result, Err(ref error) if error.code == 69));
    assert_eq!(
        fs::read_to_string(root.join("agent/coder.d/status")).unwrap_or_default(),
        "busy\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/coder.d/pid")).unwrap_or_default(),
        "100\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/coder.d/log")).unwrap_or_default(),
        ""
    );
}

#[test]
fn agent_stop_host_fallback_validates_grandchildren_before_any_stop_write() {
    let root = clean_test_dir("ctx-agent-stop-invalid-grandchild");
    let outside = clean_test_dir("ctx-agent-stop-invalid-grandchild-outside");
    create_agent_fixture(&root, "coder", "agent:base", "busy", "100");
    create_agent_fixture(&root, "helper", "agent:coder", "busy", "101");
    create_agent_fixture(&root, "nested", "agent:helper", "ready", "102");
    for name in ["coder", "helper", "nested"] {
        write_text_file(&root.join(format!("agent/{name}.d/log")), "");
    }
    assert!(fs::remove_file(root.join("agent/nested.d/life")).is_ok());
    assert!(
        symlink(
            outside.join("life"),
            root.join("agent/nested.d/life")
        )
        .is_ok()
    );

    let result = agent_stop(&root, "coder");

    assert!(matches!(result, Err(ref error) if error.code == 69));
    for (name, status, pid) in [
        ("coder", "busy\n", "100\n"),
        ("helper", "busy\n", "101\n"),
        ("nested", "ready\n", "102\n"),
    ] {
        assert_eq!(
            fs::read_to_string(root.join(format!("agent/{name}.d/status")))
                .unwrap_or_default(),
            status,
            "{name} status"
        );
        assert_eq!(
            fs::read_to_string(root.join(format!("agent/{name}.d/pid")))
                .unwrap_or_default(),
            pid,
            "{name} pid"
        );
        assert_eq!(
            fs::read_to_string(root.join(format!("agent/{name}.d/log")))
                .unwrap_or_default(),
            "",
            "{name} log"
        );
    }
}

#[test]
fn agent_stop_host_fallback_rejects_malformed_grandchild_parent_before_writes() {
    let root = clean_test_dir("ctx-agent-stop-invalid-grandchild-parent");
    create_agent_fixture(&root, "coder", "agent:base", "busy", "100");
    create_agent_fixture(&root, "helper", "agent:coder", "busy", "101");
    create_agent_fixture(&root, "nested", "agent:helper", "ready", "102");
    for name in ["coder", "helper", "nested"] {
        write_text_file(&root.join(format!("agent/{name}.d/log")), "");
    }
    write_text_file(&root.join("agent/nested.d/parent"), "session:default\n");

    let result = agent_stop(&root, "coder");

    assert!(matches!(
        result,
        Err(ref error)
            if error.code == 2 && error.message.contains("invalid agent parent for nested")
    ));
    for (name, status) in [
        ("coder", "busy\n"),
        ("helper", "busy\n"),
        ("nested", "ready\n"),
    ] {
        assert_eq!(
            fs::read_to_string(root.join(format!("agent/{name}.d/status")))
                .unwrap_or_default(),
            status,
            "{name} status"
        );
        assert_eq!(
            fs::read_to_string(root.join(format!("agent/{name}.d/log")))
                .unwrap_or_default(),
            "",
            "{name} log"
        );
    }
}

#[test]
fn agent_stop_host_fallback_rejects_ownership_cycle_before_any_stop_write() {
    let root = clean_test_dir("ctx-agent-stop-cycle");
    create_agent_fixture(&root, "coder", "agent:nested", "busy", "100");
    create_agent_fixture(&root, "nested", "agent:coder", "ready", "102");
    for name in ["coder", "nested"] {
        write_text_file(&root.join(format!("agent/{name}.d/log")), "");
    }

    let result = agent_stop(&root, "coder");

    assert!(matches!(
        result,
        Err(ref error)
            if error.code == 69 && error.message.contains("agent stop ownership cycle")
    ));
    assert_eq!(
        fs::read_to_string(root.join("agent/coder.d/status")).unwrap_or_default(),
        "busy\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/nested.d/status")).unwrap_or_default(),
        "ready\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/coder.d/log")).unwrap_or_default(),
        ""
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/nested.d/log")).unwrap_or_default(),
        ""
    );
}

#[test]
fn agent_stop_host_fallback_preflights_child_result_channel_before_control_writes() {
    let root = clean_test_dir("ctx-agent-stop-unwritable-child-result");
    create_agent_fixture(&root, "coder", "agent:base", "busy", "100");
    create_agent_fixture(
        &root,
        "helper",
        "agent:coder session:default run:r1",
        "busy",
        "101",
    );
    for name in ["coder", "helper"] {
        write_text_file(&root.join(format!("agent/{name}.d/log")), "");
    }
    let session = ctx_home(&root)
        .unwrap_or_default()
        .join("agent/coder/session/default");
    create_complete_session_layout(&session);
    let child = session.join("context/child/work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "helper\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "active\n");
    write_text_file(&child.join("handoff.md"), "Task: test preflight.\n");
    write_text_file(&child.join("result.md"), "");
    write_text_file(&child.join("refs.jsonl"), "");
    let metadata = fs::metadata(child.join("result.md"));
    assert!(metadata.is_ok(), "{metadata:?}");
    let Ok(metadata) = metadata else {
        return;
    };
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o444);
    assert!(fs::set_permissions(child.join("result.md"), permissions).is_ok());

    let result = agent_stop(&root, "coder");

    assert!(matches!(result, Err(ref error) if error.code == 69));
    assert_eq!(
        fs::read_to_string(root.join("agent/coder.d/status")).unwrap_or_default(),
        "busy\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/helper.d/status")).unwrap_or_default(),
        "busy\n"
    );
    assert_eq!(
        fs::read_to_string(child.join("status")).unwrap_or_default(),
        "active\n"
    );
}

#[test]
fn agent_stop_host_fallback_cancels_sessionless_owned_child_agent() {
    let root = clean_test_dir("ctx-agent-stop-host-fallback-sessionless-child");
    create_agent_fixture(&root, "coder", "agent:base", "busy", "100");
    create_agent_fixture(&root, "helper", "agent:coder", "busy", "101");
    write_text_file(&root.join("agent/coder.d/log"), "");
    write_text_file(&root.join("agent/helper.d/log"), "");
    let session = ctx_home(&root)
        .unwrap_or_default()
        .join("agent/coder/session/feature");
    create_complete_session_layout(&session);
    let child = session.join("context/child/work-live");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "helper\n");
    write_text_file(&child.join("session"), "feature\n");
    write_text_file(&child.join("status"), "active\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(&child.join("result.md"), "");
    write_text_file(&child.join("refs.jsonl"), "");

    assert_eq!(agent_stop(&root, "coder"), Ok(ExitCode::SUCCESS));

    assert_eq!(
        fs::read_to_string(root.join("agent/helper.d/status")).unwrap_or_default(),
        "dead\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/helper.d/pid")).unwrap_or_default(),
        "\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/helper.d/log")).unwrap_or_default(),
        "{\"type\":\"agent.stop\",\"agent\":\"helper\",\"status\":\"cancelled\"}\n"
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
