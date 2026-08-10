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
        "openai/gpt-5.6",
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
                && args.models == ["openai/gpt-5.6".to_owned()]
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

    let inspect = cmd!("agent", "inspect", "reviewer", "--session", "test");
    assert!(matches!(
        inspect,
        Ok(Command::Agent(AgentArgs::Inspect {
            ref name,
            session: Some(ref session)
        })) if name == "reviewer" && session == "test"
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
fn agent_inspect_projects_definition_instance_session_and_model() {
    let root = clean_test_dir("ctx-agent-inspect");
    let ensured = ensure_reference_tree(&root);
    assert!(ensured.is_ok(), "reference tree: {ensured:?}");
    ensure_runtime_model_fixture(&root);
    let session = ctx_home(&root)
        .unwrap_or_default()
        .join("agent/coder/session/default");
    assert!(fs::create_dir_all(&session).is_ok());
    write_text_file(&session.join("state"), "idle\n");
    write_text_file(&session.join("cwd"), "/workspace\n");

    let lines = agent_inspect_lines(&root, "coder", Some("default"));
    assert!(lines.is_ok(), "agent inspect: {lines:?}");
    let lines = lines.unwrap_or_default();
    assert!(lines.iter().any(|line| line.ends_with("/agent/coder")));
    assert!(lines.iter().any(|line| line == "session.state=idle"));
    assert!(lines.iter().any(|line| line == "model.name=main"));
    assert!(lines.iter().any(|line| line.starts_with("model.cap=")));
    assert!(lines.iter().any(|line| line.starts_with("tools=")));
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
        "openai/gpt-5.6",
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
            "{\"name\":\"reviewer\",\"label\":\"reviewer_t\",\"model\":[\"openai/gpt-5.6\"],\"tools\":[\"fs.read\"],\"shared\":{\"project-a\":[\"read\"]},\"mount\":[[\"/work\",\"/work\",\"ro\"]]}".to_owned()
        )
    );
}

#[test]
fn agent_new_request_json_rejects_protected_mount_targets() {
    let command = cmd!(
        "agent", "new", "reviewer", "--mount", "/tmp/source", "/ctx", "rw",
    );
    assert!(matches!(command, Ok(Command::Agent(AgentArgs::New(_)))));
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };
    let error = agent_new_request_json(&args);
    assert!(error.is_err());
    let Err(error) = error else {
        return;
    };
    assert!(
        error
            .message
            .contains("agent mount target cannot replace sandbox system paths")
    );
}

#[test]
fn agent_new_rejects_control_characters_in_mount_paths() {
    let root = clean_test_dir("ctx-agent-new-control-mount");
    assert!(fs::create_dir_all(&root).is_ok());
    let cases = [
        ("source-newline", "/tmp/source\nbad", "/workspace"),
        ("source-escape", "/tmp/source\x1bbad", "/workspace"),
        ("target-newline", "/tmp/source", "/workspace\nbad"),
        ("target-escape", "/tmp/source", "/workspace\x1bbad"),
    ];

    for (name, source, target) in cases {
        let command = parse_command(vec![
            "agent".to_owned(),
            "new".to_owned(),
            name.to_owned(),
            "--mount".to_owned(),
            source.to_owned(),
            target.to_owned(),
            "rw".to_owned(),
        ]);
        assert!(matches!(command, Ok(Command::Agent(AgentArgs::New(_)))));
        let Ok(Command::Agent(AgentArgs::New(args))) = command else {
            return;
        };
        assert!(agent_new(&root, &args).is_err(), "accepted mount for {name}");
        assert!(!root.join(format!("agent/{name}.d")).exists());
    }
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
        "api.test/gpt-5.6",
        "--tool",
        "tsh.config",
    );
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };

    assert_eq!(agent_new(&root, &args), Ok(ExitCode::SUCCESS));
    assert!(root.join("agent/worker").is_file());
    let executable = fs::read_to_string(root.join("agent/worker")).unwrap_or_default();
    assert!(executable.contains("# cortexfs.object=agent\n"));
    assert!(executable.contains("# cortexfs.name=worker\n"));
    assert!(executable.contains("exec '/ctx/bin/cortexfs-object-runner' \"$0\" \"$@\"\n"));
    assert!(!executable.contains("/model/"));
    assert!(!executable.contains("CTX_AGENT_SYSTEM="));
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
    let mut expected_groups = nix::unistd::getgroups()
        .unwrap_or_default()
        .into_iter()
        .map(nix::unistd::Gid::as_raw)
        .collect::<Vec<_>>();
    expected_groups.sort_unstable();
    expected_groups.dedup();
    let mut expected_groups_text = expected_groups
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let suffix = if expected_groups_text.is_empty() { "" } else { "\n" };
    expected_groups_text.push_str(suffix);
    assert_eq!(
        fs::read_to_string(root.join("agent/worker.d/groups")).unwrap_or_default(),
        expected_groups_text
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/worker.d/parent")).unwrap_or_default(),
        "agent:coder\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/worker.d/model"))
            .unwrap_or_default(),
        "api.test/gpt-5.6\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/worker.d/policy"))
            .unwrap_or_default(),
        "allow worker_t model:api.test/gpt-5.6 use\nallow worker_t tool:tsh execute\nallow worker_t network:default connect\nallow worker_t tool:tsh.config execute\n"
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
fn agent_runtime_gate_requires_matching_projection_session_and_run() {
    let source = clean_test_dir("agent-runtime-gate-source");
    assert!(ensure_reference_tree(&source).is_ok());
    ensure_runtime_model_fixture(&source);
    let projection = clean_test_dir("agent-runtime-gate-projection");
    let source_control = source.join("agent/coder.d");
    let projected_control = projection.join("agent/coder.d");
    assert!(fs::create_dir_all(&projected_control).is_ok());
    for file in [
        "owner", "uid", "gid", "groups", "label", "iso", "root", "cwd", "env", "path",
        "mount", "model", "policy", "parent", "life",
    ] {
        assert!(fs::copy(source_control.join(file), projected_control.join(file)).is_ok());
    }
    ensure_runtime_model_fixture(&projection);
    let session = source.join("home/1000/agent/coder/session/runtime-test");
    assert!(fs::create_dir_all(&session).is_ok());
    write_text_file(&session.join("current_run"), "run-1\n");

    assert!(agent_runtime_context_matches_values(
        &projection,
        &source,
        &projection,
        "coder",
        "runtime-test",
        "run-1",
    ));
    assert_eq!(agent_lifecycle_tool_selected(&projection, true), Ok(false));
    let tool = projection.join("tool/agent.create");
    assert!(fs::create_dir_all(tool.parent().unwrap_or(&projection)).is_ok());
    write_text_file(&tool, "#!/bin/sh\nexit 0\n");
    assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());
    assert_eq!(agent_lifecycle_tool_selected(&projection, true), Ok(true));
    assert_eq!(agent_lifecycle_tool_selected(&projection, false), Ok(false));
    assert!(!agent_runtime_context_matches_values(
        &projection,
        &source,
        &projection,
        "coder",
        "missing",
        "run-1",
    ));
    assert!(!agent_runtime_context_matches_values(
        &projection,
        &source,
        &projection,
        "coder",
        "runtime-test",
        "missing",
    ));
    write_text_file(&projected_control.join("model"), "different\n");
    assert!(!agent_runtime_context_matches_values(
        &projection,
        &source,
        &projection,
        "coder",
        "runtime-test",
        "run-1",
    ));
    assert!(fs::copy(source_control.join("model"), projected_control.join("model")).is_ok());
    for file in ["iso", "root", "cwd", "env", "path"] {
        let original = fs::read(projected_control.join(file)).unwrap_or_default();
        write_text_file(&projected_control.join(file), "drift\n");
        assert!(
            !agent_runtime_context_matches_values(
                &projection,
                &source,
                &projection,
                "coder",
                "runtime-test",
                "run-1",
            ),
            "accepted drift in {file}"
        );
        assert!(fs::write(projected_control.join(file), original).is_ok());
    }
    assert!(!agent_runtime_context_matches_values(
        &projection,
        &source,
        Path::new("/different-projection"),
        "coder",
        "runtime-test",
        "run-1",
    ));
}

#[test]
fn agent_new_selects_runtime_tool_or_host_fallback_in_isolated_processes() -> std::io::Result<()> {
    const MODE: &str = "CORTEXFS_TEST_AGENT_NEW_SELECTION";
    if let Some(mode) = std::env::var_os(MODE) {
        let root = PathBuf::from(std::env::var_os("CTX_TEST_ROOT").unwrap_or_default());
        let name = if mode == "runtime" { "tool-child" } else { "host-child" };
        let parsed = parse_command(vec![
            "agent".to_owned(),
            "new".to_owned(),
            name.to_owned(),
        ]);
        let Ok(Command::Agent(AgentArgs::New(args))) = parsed else {
            return Err(std::io::Error::other("agent new did not parse"));
        };
        assert_eq!(agent_new(&root, &args), Ok(ExitCode::SUCCESS));
        return Ok(());
    }

    let source = clean_test_dir("agent-new-selection-source");
    ensure_reference_tree(&source)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    ensure_runtime_model_fixture(&source);
    let projection = clean_test_dir("agent-new-selection-projection");
    let projected_control = projection.join("agent/coder.d");
    fs::create_dir_all(&projected_control)?;
    for file in [
        "owner", "uid", "gid", "groups", "label", "iso", "root", "cwd", "env", "path",
        "mount", "model", "policy", "parent", "life",
    ] {
        fs::copy(source.join("agent/coder.d").join(file), projected_control.join(file))?;
    }
    ensure_runtime_model_fixture(&projection);
    let session = source.join("home/1000/agent/coder/session/runtime-test");
    fs::create_dir_all(&session)?;
    write_text_file(&session.join("current_run"), "run-1\n");
    let marker = projection.join("runtime-tool.marker");
    let tool = projection.join("tool/agent.create");
    fs::create_dir_all(tool.parent().unwrap_or(&projection))?;
    write_text_file(
        &tool,
        &format!("#!/bin/sh\nprintf selected > '{}'\n", marker.display()),
    );
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o755))?;

    let test_binary = std::env::current_exe()?;
    let run = |mode: &str, runtime: bool| -> std::io::Result<std::process::ExitStatus> {
        let mut command = std::process::Command::new(&test_binary);
        command
            .arg("tests::agent_new_selects_runtime_tool_or_host_fallback_in_isolated_processes")
            .arg("--exact")
            .env(MODE, mode)
            .env("CTX_TEST_ROOT", &projection)
            .env_remove("CTX_AGENT")
            .env_remove("CTX_SESSION")
            .env_remove("CTX_RUN_ID")
            .env_remove("CTX_ROOT")
            .env_remove("CTX_SOURCE");
        if runtime {
            command
                .env("CTX_AGENT", "coder")
                .env("CTX_SESSION", "runtime-test")
                .env("CTX_RUN_ID", "run-1")
                .env("CTX_ROOT", &projection)
                .env("CTX_SOURCE", &source);
        }
        command.status()
    };

    assert!(run("runtime", true)?.success());
    assert!(marker.is_file());
    assert!(!projection.join("agent/tool-child.d").exists());
    fs::remove_file(&marker)?;
    assert!(run("host", false)?.success());
    assert!(projection.join("agent/host-child.d").is_dir());
    assert!(projection.join("agent/host-child").is_file());
    assert!(!marker.exists());
    Ok(())
}
#[test]
fn child_wait_terminal_exit_codes_are_stable() {
    assert_eq!(
        child_wait_exit_code(ChildContextStatus::Done),
        ExitCode::from(0)
    );
    assert_eq!(
        child_wait_exit_code(ChildContextStatus::Error),
        ExitCode::from(1)
    );
    assert_eq!(
        child_wait_exit_code(ChildContextStatus::Cancelled),
        ExitCode::from(130)
    );
}

#[test]
fn child_wait_resolves_status_with_one_read() {
    let mut reads = 0_u8;
    let status = resolve_child_wait_status("worker", || {
        reads = reads.saturating_add(1);
        Ok(Some("done".to_owned()))
    });
    assert_eq!(status.ok(), Some(ChildContextStatus::Done));
    assert_eq!(reads, 1);
}
