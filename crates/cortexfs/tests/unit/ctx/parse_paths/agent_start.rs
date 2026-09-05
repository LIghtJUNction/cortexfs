use crate::{
    agent_socket_path, agent_start_host, agent_start_workspace_source, ensure_agent_start_session,
};

fn current_uid_for_test() -> String {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|uid| uid.trim().to_owned())
        .filter(|uid| !uid.is_empty())
        .unwrap_or_else(|| "1000".to_owned())
}

#[test]
fn system_agent_socket_uses_root_runtime_authority() {
    assert_eq!(
        cortexfs_paths::system_agent_socket_unit("worker-1"),
        "cortexfs-agent@worker-1.socket"
    );
    assert_eq!(
        cortexfs_paths::system_agent_runtime_socket("worker-1"),
        PathBuf::from("/run/cortexfs/agent/worker-1.sock")
    );
    let command = system_agent_socket_command("start", "worker-1");
    assert_eq!(command.get_program(), "/usr/bin/systemctl");
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        [
            "--no-ask-password",
            "start",
            "cortexfs-agent@worker-1.socket"
        ]
    );
}

#[test]
fn terminal_rollback_does_not_remove_system_agent_socket() {
    let root = clean_test_dir("terminal-rollback-system-agent-socket");
    let system_socket = root.join("agent.sock");
    write_text_file(&system_socket, "system-authority\n");
    rollback_agent_start_resources_with("terminal", None, &[], &[], |_unit| {}, |_unit| {});
    assert_eq!(
        fs::read_to_string(&system_socket).unwrap_or_default(),
        "system-authority\n"
    );
}

fn git_command(home: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("/usr/bin/git");
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    command
}

fn create_real_git_worktree(fixture: &Path) -> Option<(PathBuf, PathBuf, PathBuf)> {
    let source = fixture.join("workspace");
    let repository = fixture.join("repository.git");
    let home = fixture.join("home");
    fs::create_dir_all(&home).ok()?;
    git_command(&home)
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("init")
        .arg("--bare")
        .arg("-q")
        .arg(&repository)
        .status()
        .ok()?
        .success()
        .then_some(())?;
    git_command(&home)
        .arg("-C")
        .arg(&repository)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "worktree",
            "add",
            "--orphan",
            "-q",
        ])
        .arg(&source)
        .status()
        .ok()?
        .success()
        .then_some(())?;
    let git_file = fs::read_to_string(source.join(".git")).ok()?;
    let gitdir = PathBuf::from(git_file.trim().strip_prefix("gitdir: ")?);
    let commondir = repository;
    Some((source, gitdir, commondir))
}

fn run_bwrap_script(mut args: Vec<String>, script: &str) -> Option<std::process::Output> {
    let command = args.iter().position(|arg| arg == "/usr/bin/ctxterm")?;
    args.truncate(command);
    args.extend(["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()]);
    std::process::Command::new("/usr/bin/bwrap")
        .args(args)
        .output()
        .ok()
}

fn command_bwrap_args(
    root: &Path,
    args: &AgentStartArgs,
    mounts: &[AgentMount],
    view: &AgentRuntimeView,
) -> Option<Vec<String>> {
    let command = agent_start_systemd_command(
        root,
        args,
        mounts,
        view,
        Path::new(cortexfs::runtime::terminal::broker::BROKER_SOCKET),
        "cortexfs-agent-executor-test-terminal",
    );
    let start = command
        .args
        .iter()
        .position(|arg| arg == cortexfs::support::command::BWRAP)?
        .checked_add(1)?;
    command.args.get(start..).map(<[String]>::to_vec)
}

fn agent_bwrap_test_args(args: &AgentStartArgs, mounts: &[AgentMount]) -> Option<Vec<String>> {
    let root = clean_test_dir("ctx-agent-git-bwrap-args");
    ensure_reference_tree(&root).ok()?;
    ensure_runtime_model_fixture(&root);
    let view = derive_agent_runtime_view(&root, "executor").ok()?;
    command_bwrap_args(&root, args, mounts, &view)
}

fn run_agent_bwrap(
    args: &AgentStartArgs,
    mounts: &[AgentMount],
    policy: Option<&str>,
    script: &str,
) -> Option<(Vec<String>, std::process::Output)> {
    let root = clean_test_dir("ctx-agent-git-bwrap-run");
    ensure_reference_tree(&root).ok()?;
    ensure_runtime_model_fixture(&root);
    if let Some(policy) = policy {
        write_text_file(&root.join("agent").join("executor.d").join("mount"), policy);
    }
    let view = derive_agent_runtime_view(&root, "executor").ok()?;
    let bwrap = command_bwrap_args(&root, args, mounts, &view)?;
    let output = run_bwrap_script(bwrap.clone(), script)?;
    Some((bwrap, output))
}

#[test]
fn agent_terminal_socket_uses_session_terminal_main_socket() {
    let root = clean_test_dir("ctx-agent-terminal-socket");
    let socket = agent_terminal_socket(&root, "executor", "test");
    assert_eq!(
        socket,
        Ok(root
            .join("home")
            .join(current_uid_for_test())
            .join("agent")
            .join("executor")
            .join("session")
            .join("test")
            .join("terminal")
            .join("main.sock"))
    );
}

#[test]
fn agent_start_builds_sandboxed_terminal_command() {
    let root = clean_test_dir("ctx-agent-start-bwrap-view");
    assert!(ensure_reference_tree(&root).is_ok());
    ensure_runtime_model_fixture(&root);
    write_text_file(
        &root.join("agent").join("executor.d").join("env"),
        "CTX_ROOT=/bad\nCTX_PROVIDER_CONFIG_DIR=/bad/providers.d\n",
    );
    let view = derive_agent_runtime_view(&root, "executor");
    assert!(view.is_ok(), "reference executor view: {view:?}");
    let Ok(view) = view else {
        return;
    };
    let args = AgentStartArgs {
        name: "executor".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: vec![AgentMount {
            source: "/repo".to_owned(),
            target: "/workspace".to_owned(),
            mode: "rw".to_owned(),
        }],
    };
    let cli_mounts = vec![AgentMount {
        source: "/repo".to_owned(),
        target: "/workspace".to_owned(),
        mode: "rw".to_owned(),
    }];
    let Some(bwrap) = command_bwrap_args(&root, &args, &cli_mounts, &view) else {
        return;
    };
    assert!(contains_arg_triplet(
        &bwrap,
        "--ro-bind",
        &root.display().to_string(),
        "/ctx"
    ));
    assert!(contains_arg_triplet(
        &bwrap,
        "--bind",
        &root
            .join("home")
            .join("1000")
            .join("agent")
            .join("executor")
            .display()
            .to_string(),
        "/home/agent"
    ));
    assert!(contains_arg_triplet(
        &bwrap,
        "--setenv",
        "CTX_AGENT_ROLE",
        "worker"
    ));
    assert!(contains_arg_triplet(
        &bwrap,
        "--setenv",
        "CTX_PROVIDER_CONFIG_DIR",
        "/ctx/shared/providers.d"
    ));
    assert!(!bwrap.iter().any(|arg| arg == "/bad/providers.d"));
    assert!(contains_arg_triplet(
        &bwrap,
        "--setenv",
        "CTX_AGENT_MODEL",
        "main"
    ));
    assert!(contains_arg_triplet(
        &bwrap,
        "--setenv",
        "CTX_AGENT_LIFE",
        "owned"
    ));
    assert!(contains_arg_triplet(
        &bwrap,
        "--bind",
        "/repo",
        "/workspace"
    ));
    assert!(bwrap.contains(&"--unshare-net".to_owned()));
    assert!(bwrap.contains(&"--as-pid-1".to_owned()));
    assert!(bwrap.contains(&"--new-session".to_owned()));
    assert!(contains_arg_pair(&bwrap, "--cap-drop", "ALL"));
    assert!(bwrap.contains(&"--unshare-uts".to_owned()));
    assert!(!bwrap.iter().any(|arg| arg == "--hostname"));
    assert!(contains_arg_pair(&bwrap, "--dir", "/home"));
    assert!(contains_empty_startup_bind(&bwrap, "/etc/profile"));
    assert!(contains_empty_startup_bind(&bwrap, "/etc/bash.bashrc"));
    assert!(contains_empty_startup_bind(&bwrap, "/etc/environment"));
    assert!(contains_arg_pair(&bwrap, "--tmpfs", "/etc/profile.d"));
    assert!(contains_arg_pair(&bwrap, "--chdir", "/workspace"));
    assert!(contains_arg_pair(&bwrap, "--broker", "executor"));
    assert!(bwrap.contains(&"cortexfs-agent-executor-test-terminal".to_owned()));
    assert_eq!(
        contains_arg_triplet(
            &bwrap,
            "--ro-bind",
            cortexfs::runtime::terminal::broker::BROKER_SOCKET,
            cortexfs::runtime::terminal::broker::BROKER_SOCKET,
        ),
        Path::new(cortexfs::runtime::terminal::broker::BROKER_SOCKET).exists()
    );
    assert!(!bwrap.iter().any(|arg| arg == "--listen"));
    assert_eq!(bwrap.last().map(String::as_str), Some("/ctx/bin/tsh"));
}

#[test]
fn agent_start_rejects_mount_outside_runtime_view() {
    let root = clean_test_dir("ctx-agent-start-mount-authority");
    assert!(ensure_reference_tree(&root).is_ok());
    ensure_runtime_model_fixture(&root);
    let args = AgentStartArgs {
        name: "executor".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: false,
        mounts: vec![AgentMount {
            source: "/host-secret".to_owned(),
            target: "/workspace".to_owned(),
            mode: "rw".to_owned(),
        }],
    };
    let result = agent_start_host(&root, &args);
    assert!(
        matches!(&result, Err(error) if error.message.contains("exceeds agent mount policy")),
        "{result:?}"
    );
}

#[test]
fn agent_start_default_workspace_masks_git_directory_until_explicitly_mounted() {
    let source = clean_test_dir("ctx-agent-start-git-ro");
    let home = clean_test_dir("ctx-agent-start-git-home");
    assert!(fs::create_dir_all(&home).is_ok());
    assert!(
        git_command(&home)
            .arg("-c")
            .arg("core.hooksPath=/dev/null")
            .arg("init")
            .arg("-q")
            .arg(&source)
            .status()
            .is_ok_and(|status| status.success())
    );
    write_text_file(&source.join(".git").join("host-marker"), "host metadata\n");
    let args = AgentStartArgs {
        name: "executor".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: Vec::new(),
    };
    let mounts = agent_start_mounts_with_default_source(&args, &source);
    assert_eq!(
        mounts,
        vec![AgentMount {
            source: source.display().to_string(),
            target: "/workspace".to_owned(),
            mode: "rw".to_owned(),
        }]
    );

    let result = run_agent_bwrap(
        &args,
        &mounts,
        None,
        "test ! -e /workspace/.git/host-marker && touch /workspace/.git/sandbox-only",
    );
    assert!(result.is_some(), "run masked git directory sandbox");
    let Some((bwrap, output)) = result else {
        return;
    };
    assert!(contains_arg_pair(&bwrap, "--tmpfs", "/workspace/.git"));
    assert!(
        !bwrap
            .iter()
            .any(|arg| arg == &source.join(".git").display().to_string())
    );
    assert!(output.status.success(), "masked sandbox failed: {output:?}");
    assert!(!source.join(".git").join("sandbox-only").exists());

    let explicit_args = AgentStartArgs {
        name: "executor".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: vec![AgentMount {
            source: source.join(".git").display().to_string(),
            target: "/workspace/.git".to_owned(),
            mode: "ro".to_owned(),
        }],
    };
    let explicit_mounts = agent_start_mounts_with_default_source(&explicit_args, &source);
    let result = run_agent_bwrap(
        &explicit_args,
        &explicit_mounts,
        None,
        "/usr/bin/git -C /workspace status --porcelain >/dev/null && ! /usr/bin/touch /workspace/.git/blocked 2>/dev/null",
    );
    assert!(result.is_some(), "run explicit git directory sandbox");
    let Some((explicit_bwrap, output)) = result else {
        return;
    };
    assert!(!contains_arg_pair(
        &explicit_bwrap,
        "--tmpfs",
        "/workspace/.git"
    ));
    assert!(contains_arg_triplet(
        &explicit_bwrap,
        "--ro-bind",
        source.join(".git").to_str().unwrap_or_default(),
        "/workspace/.git"
    ));
    assert!(
        output.status.success(),
        "explicit sandbox failed: {output:?}"
    );
    assert!(!source.join(".git").join("blocked").exists());
}

#[test]
fn agent_start_git_file_does_not_authorize_external_mount() {
    let source = clean_test_dir("ctx-agent-start-git-file");
    let external = clean_test_dir("ctx-agent-start-git-external");
    assert!(fs::create_dir_all(&external).is_ok());
    write_text_file(
        &source.join(".git"),
        &format!("gitdir: {}\n", external.display()),
    );

    let args = AgentStartArgs {
        name: "executor".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: Vec::new(),
    };
    let mounts = agent_start_mounts_with_default_source(&args, &source);
    assert_eq!(
        mounts,
        vec![AgentMount {
            source: source.display().to_string(),
            target: "/workspace".to_owned(),
            mode: "rw".to_owned(),
        }]
    );
    let bwrap = agent_bwrap_test_args(&args, &mounts);
    assert!(bwrap.is_some(), "build git file mask args");
    let Some(bwrap) = bwrap else {
        return;
    };
    assert!(contains_arg_triplet(
        &bwrap,
        "--ro-bind",
        "/dev/null",
        "/workspace/.git"
    ));
    assert!(
        !bwrap
            .iter()
            .any(|arg| arg == &external.display().to_string())
    );

    assert!(fs::remove_file(source.join(".git")).is_ok());
    assert!(std::os::unix::fs::symlink(&external, source.join(".git")).is_ok());
    assert!(contains_arg_triplet(
        &bwrap,
        "--ro-bind",
        "/dev/null",
        "/workspace/.git"
    ));
    assert!(
        !bwrap
            .iter()
            .any(|arg| arg == &source.join(".git").display().to_string())
    );
}

#[test]
fn agent_start_policy_git_overlays_keep_declared_order() {
    let source = clean_test_dir("ctx-agent-start-policy-git");
    let home = clean_test_dir("ctx-agent-start-policy-home");
    let decoy = clean_test_dir("ctx-agent-start-policy-decoy");
    assert!(fs::create_dir_all(&home).is_ok());
    assert!(fs::create_dir_all(&decoy).is_ok());
    write_text_file(&decoy.join("decoy"), "decoy\n");
    assert!(
        git_command(&home)
            .arg("-c")
            .arg("core.hooksPath=/dev/null")
            .arg("init")
            .arg("-q")
            .arg(&source)
            .status()
            .is_ok_and(|status| status.success())
    );
    let args = AgentStartArgs {
        name: "executor".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: Vec::new(),
    };
    let mounts = agent_start_mounts_with_default_source(&args, &source);
    let git = source.join(".git").display().to_string();
    let decoy = decoy.display().to_string();
    let rw_ro = format!(
        "/ctx\t/ctx\tro\trbind,nosuid,nodev\n{decoy}\t/workspace/.git\trw\trbind,nosuid,nodev\n{git}\t/workspace/.git\tro\trbind,nosuid,nodev\n"
    );
    let result = run_agent_bwrap(
        &args,
        &mounts,
        Some(&rw_ro),
        "/usr/bin/git -C /workspace status --porcelain >/dev/null && ! /usr/bin/touch /workspace/.git/blocked 2>/dev/null",
    );
    assert!(result.is_some(), "run rw then ro policy overlays");
    let Some((bwrap, output)) = result else {
        return;
    };
    let rw = bwrap
        .windows(3)
        .position(|window| window == ["--bind", decoy.as_str(), "/workspace/.git"]);
    let ro = bwrap
        .windows(3)
        .position(|window| window == ["--ro-bind", git.as_str(), "/workspace/.git"]);
    assert!(rw.zip(ro).is_some_and(|(rw, ro)| rw < ro));
    assert!(output.status.success(), "rw-ro policy failed: {output:?}");
    assert!(!source.join(".git").join("blocked").exists());

    let ro_rw = format!(
        "/ctx\t/ctx\tro\trbind,nosuid,nodev\n{git}\t/workspace/.git\tro\trbind,nosuid,nodev\n{decoy}\t/workspace/.git\trw\trbind,nosuid,nodev\n"
    );
    let result = run_agent_bwrap(
        &args,
        &mounts,
        Some(&ro_rw),
        "test -e /workspace/.git/decoy && /usr/bin/touch /workspace/.git/policy-write",
    );
    assert!(result.is_some(), "run ro then rw policy overlays");
    let Some((bwrap, output)) = result else {
        return;
    };
    let ro = bwrap
        .windows(3)
        .position(|window| window == ["--ro-bind", git.as_str(), "/workspace/.git"]);
    let rw = bwrap
        .windows(3)
        .position(|window| window == ["--bind", decoy.as_str(), "/workspace/.git"]);
    assert!(ro.zip(rw).is_some_and(|(ro, rw)| ro < rw));
    assert!(output.status.success(), "ro-rw policy failed: {output:?}");
    assert!(Path::new(&decoy).join("policy-write").exists());
}

#[test]
fn agent_start_real_worktree_requires_explicit_metadata_mounts() {
    let fixture = clean_test_dir("ctx-agent-start-real-worktree");
    let fixture = create_real_git_worktree(&fixture);
    assert!(fixture.is_some(), "create real git worktree fixture");
    let Some((source, git_dir, common_dir)) = fixture else {
        return;
    };
    let args = AgentStartArgs {
        name: "executor".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: Vec::new(),
    };

    let mounts = agent_start_mounts_with_default_source(&args, &source);

    assert_eq!(
        mounts,
        vec![AgentMount {
            source: source.display().to_string(),
            target: "/workspace".to_owned(),
            mode: "rw".to_owned(),
        }]
    );

    let result = run_agent_bwrap(
        &args,
        &mounts,
        None,
        "! /usr/bin/git -C /workspace rev-parse --git-dir >/dev/null 2>&1",
    );
    assert!(result.is_some(), "run masked worktree sandbox");
    let Some((bwrap, output)) = result else {
        return;
    };
    assert!(contains_arg_triplet(
        &bwrap,
        "--bind",
        source.to_str().unwrap_or_default(),
        "/workspace"
    ));
    assert!(!contains_arg_triplet(
        &bwrap,
        "--ro-bind",
        git_dir.to_str().unwrap_or_default(),
        git_dir.to_str().unwrap_or_default()
    ));
    assert!(!contains_arg_triplet(
        &bwrap,
        "--ro-bind",
        common_dir.to_str().unwrap_or_default(),
        common_dir.to_str().unwrap_or_default()
    ));
    assert!(contains_arg_triplet(
        &bwrap,
        "--ro-bind",
        "/dev/null",
        "/workspace/.git"
    ));
    assert!(output.status.success(), "masked worktree failed: {output:?}");

    let explicit_args = AgentStartArgs {
        name: "executor".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: vec![
            AgentMount {
                source: source.join(".git").display().to_string(),
                target: "/workspace/.git".to_owned(),
                mode: "ro".to_owned(),
            },
            AgentMount {
                source: git_dir.display().to_string(),
                target: git_dir.display().to_string(),
                mode: "ro".to_owned(),
            },
            AgentMount {
                source: common_dir.display().to_string(),
                target: common_dir.display().to_string(),
                mode: "ro".to_owned(),
            },
        ],
    };
    let explicit_mounts = agent_start_mounts_with_default_source(&explicit_args, &source);
    let result = run_agent_bwrap(
        &explicit_args,
        &explicit_mounts,
        None,
        "/usr/bin/git -C /workspace rev-parse --git-common-dir >/dev/null && gitdir=$(/usr/bin/git -C /workspace rev-parse --git-dir) && ! /usr/bin/touch \"$gitdir/blocked\" 2>/dev/null",
    );
    assert!(result.is_some(), "run explicit worktree sandbox");
    let Some((explicit_bwrap, output)) = result else {
        return;
    };
    assert!(contains_arg_triplet(
        &explicit_bwrap,
        "--ro-bind",
        source.join(".git").to_str().unwrap_or_default(),
        "/workspace/.git"
    ));
    assert!(contains_arg_triplet(
        &explicit_bwrap,
        "--ro-bind",
        git_dir.to_str().unwrap_or_default(),
        git_dir.to_str().unwrap_or_default()
    ));
    assert!(contains_arg_triplet(
        &explicit_bwrap,
        "--ro-bind",
        common_dir.to_str().unwrap_or_default(),
        common_dir.to_str().unwrap_or_default()
    ));
    assert!(!contains_arg_triplet(
        &explicit_bwrap,
        "--ro-bind",
        "/dev/null",
        "/workspace/.git"
    ));
    assert!(output.status.success(), "explicit worktree failed: {output:?}");
    assert!(!git_dir.join("blocked").exists());
}

#[test]
fn agent_start_maps_ctx_mount_sources_to_selected_root() {
    let root = clean_test_dir("ctx-agent-start-alt-root-mount-source");

    assert_eq!(
        agent_host_mount_source(&root, "/ctx"),
        root.display().to_string()
    );
    assert_eq!(
        agent_host_mount_source(&root, "/ctx/home/1000/agent/executor"),
        root.join("home")
            .join("1000")
            .join("agent")
            .join("executor")
            .display()
            .to_string()
    );
    assert_eq!(
        agent_host_mount_source(&root, "/host/input"),
        "/host/input".to_owned()
    );
}

#[test]
fn agent_start_maps_host_cwd_to_sandbox_mount_target() {
    let source = clean_test_dir("ctx-agent-start-host-cwd");
    let subdir = source.join("nested");
    assert!(fs::create_dir_all(&subdir).is_ok());
    let args = AgentStartArgs {
        name: "executor".to_owned(),
        session: "test".to_owned(),
        cwd: subdir.display().to_string(),
        default_workspace: true,
        mounts: Vec::new(),
    };
    let mounts = agent_start_mounts_with_default_source(&args, &source);

    assert_eq!(agent_start_sandbox_cwd(&args, &mounts), "/workspace/nested");
}

#[test]
fn agent_start_records_ready_status_and_start_event() {
    let root = clean_test_dir("ctx-agent-start-record-state");
    let control = root.join("agent/scratch.d");
    create_agent_fixture(&root, "scratch", "agent:base", "start", "");
    write_text_file(&control.join("log"), "");
    let args = AgentStartArgs {
        name: "scratch".to_owned(),
        session: "default".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: Vec::new(),
    };

    let facts = [
        ("model", "main"),
        ("life", "owned"),
        ("role", "worker"),
        ("uid", "1000"),
        ("gid", "100"),
        ("groups", "10 20"),
    ];
    let identity = AgentUnixIdentity::new(
        nix::unistd::geteuid().as_raw(),
        nix::unistd::getegid().as_raw(),
        nix::unistd::getgroups()
            .unwrap_or_default()
            .into_iter()
            .map(nix::unistd::Gid::as_raw),
    );
    assert_eq!(
        record_agent_start_state(
            &root,
            &args,
            &identity,
            "cortexfs-agent-scratch-default",
            &facts,
            Some("abc123")
        ),
        Ok(())
    );
    assert_eq!(
        fs::read_to_string(control.join("status")).unwrap_or_default(),
        "ready\n"
    );
    assert_eq!(
        fs::read_to_string(control.join("pid")).unwrap_or_default(),
        "\n"
    );
    assert_eq!(
        fs::read_to_string(control.join("log")).unwrap_or_default(),
        "{\"type\":\"agent.start\",\"agent\":\"scratch\",\"session\":\"default\",\"unit\":\"cortexfs-agent-scratch-default\",\"model\":\"main\",\"life\":\"owned\",\"role\":\"worker\",\"uid\":\"1000\",\"gid\":\"100\",\"groups\":\"10 20\",\"status\":\"ready\",\"invocation\":\"abc123\"}\n"
    );
}

#[test]
fn agent_start_prepares_session_workspace_hint() {
    let root = clean_test_dir("ctx-agent-start-session-workspace");
    let workspace = clean_test_dir("ctx-agent-start-session-workspace-source");
    assert!(ensure_reference_tree(&root).is_ok());
    ensure_runtime_model_fixture(&root);
    let view = derive_agent_runtime_view(&root, "executor");
    assert!(view.is_ok(), "reference executor view: {view:?}");
    let Ok(view) = view else {
        return;
    };
    let args = AgentStartArgs {
        name: "executor".to_owned(),
        session: "selfedit".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: false,
        mounts: vec![AgentMount {
            source: workspace.display().to_string(),
            target: "/workspace".to_owned(),
            mode: "rw".to_owned(),
        }],
    };
    let mounts = agent_start_mounts_with_default_source(&args, &root);
    let cwd = agent_start_sandbox_cwd(&args, &mounts);
    let workspace_hint = agent_start_workspace_source(&mounts);

    assert_eq!(
        workspace_hint.as_deref(),
        Some(workspace.to_str().unwrap_or_default())
    );
    assert_eq!(
        ensure_agent_start_session(&root, &args, &view, &cwd, workspace_hint.as_deref()),
        Ok(())
    );

    let session = root
        .join("home")
        .join(current_uid_for_test())
        .join("agent")
        .join("executor")
        .join("session")
        .join("selfedit");
    assert_eq!(
        fs::read_to_string(session.join("cwd")).unwrap_or_default(),
        "/workspace\n"
    );
    assert_eq!(
        fs::read_to_string(session.join("workspace")).unwrap_or_default(),
        format!("{}\n", workspace.display())
    );
    assert_eq!(
        fs::read_to_string(session.join("state")).unwrap_or_default(),
        "idle\n"
    );
}

#[test]
fn systemctl_main_pid_parser_ignores_missing_pid() {
    assert_eq!(parse_main_pid("0\n"), None);
    assert_eq!(parse_main_pid("12345\n"), Some(12345));
}

#[test]
fn agent_start_default_workspace_does_not_remount_symlinked_git() {
    let source = clean_test_dir("ctx-agent-start-git-symlink");
    let target = clean_test_dir("ctx-agent-start-git-symlink-target");
    assert!(fs::create_dir_all(&source).is_ok());
    assert!(fs::create_dir_all(&target).is_ok());
    assert!(std::os::unix::fs::symlink(&target, source.join(".git")).is_ok());
    let args = AgentStartArgs {
        name: "executor".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: Vec::new(),
    };

    let mounts = agent_start_mounts_with_default_source(&args, &source);
    assert_eq!(
        mounts,
        vec![AgentMount {
            source: source.display().to_string(),
            target: "/workspace".to_owned(),
            mode: "rw".to_owned(),
        }]
    );
}

#[test]
fn agent_mount_validation_rejects_protected_sandbox_targets() {
    for target in [
        "/",
        "/usr",
        "/usr/local",
        "/etc",
        "/bin",
        "/lib",
        "/lib64",
        "/run",
        "/home",
        "/dev",
        "/proc",
        "/ctx",
        "/ctx/bin",
        "/usr/../ctx",
        "/workspace/../usr/bin",
    ] {
        let mount = AgentMount {
            source: "/tmp/source".to_owned(),
            target: target.to_owned(),
            mode: "rw".to_owned(),
        };
        assert!(
            require_agent_mount(&mount).is_err(),
            "target should be rejected: {target}"
        );
    }
}

#[test]
fn agent_mount_validation_allows_workspace_subtrees() {
    let mount = AgentMount {
        source: "/tmp/source".to_owned(),
        target: "/workspace/project".to_owned(),
        mode: "rw".to_owned(),
    };

    assert!(require_agent_mount(&mount).is_ok());
}

#[test]
fn agent_start_no_default_workspace_does_not_guess_git_mount() {
    let source = clean_test_dir("ctx-agent-start-no-default-git");
    assert!(fs::create_dir_all(source.join(".git")).is_ok());
    let args = AgentStartArgs {
        name: "executor".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: false,
        mounts: Vec::new(),
    };

    let mounts = agent_start_mounts_with_default_source(&args, &source);
    assert!(mounts.is_empty());
}

#[test]
fn agent_start_systemd_command_uses_sanitized_environment() {
    let root = clean_test_dir("ctx-agent-start-systemd-view");
    assert!(ensure_reference_tree(&root).is_ok());
    ensure_runtime_model_fixture(&root);
    let view = derive_agent_runtime_view(&root, "executor");
    assert!(view.is_ok(), "reference executor view: {view:?}");
    let Ok(view) = view else {
        return;
    };
    let args = AgentStartArgs {
        name: "executor".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: Vec::new(),
    };
    let socket = PathBuf::from("/ctx/home/1000/agent/executor/session/test/terminal/main.sock");
    let cli_mounts = agent_start_mounts_with_default_source(&args, Path::new("/repo"));
    let command = agent_start_systemd_command(
        &root,
        &args,
        &cli_mounts,
        &view,
        &socket,
        "cortexfs-agent-executor-test-terminal",
    );
    assert!(
        command.program == "/usr/bin/systemd-run"
            && command.args.contains(&"--user".to_owned())
            && command
                .args
                .contains(&"--property=Restart=always".to_owned())
            && command
                .args
                .contains(&"--property=RestartSec=250ms".to_owned())
            && command
                .args
                .contains(&"--property=MemoryMax=1G".to_owned())
            && command
                .args
                .contains(&"--property=CPUQuota=200%".to_owned())
            && command
                .args
                .contains(&"--property=TasksMax=256".to_owned())
            && command
                .args
                .contains(&"--property=OOMPolicy=stop".to_owned())
            && command.args.contains(&"-i".to_owned())
            && command.args.contains(&"PATH=/usr/bin:/bin".to_owned())
            && command.args.contains(&"/usr/bin/bwrap".to_owned())
            && contains_arg_pair(&command.args, "--clearenv", "--setenv")
            && contains_arg_triplet(&command.args, "--setenv", "CTX_ROOT", "/ctx")
            && contains_arg_triplet(&command.args, "--setenv", "CTX_HOME", "/ctx/home/1000")
            && contains_arg_triplet(&command.args, "--setenv", "CTX_AGENT", "executor")
            && contains_arg_triplet(&command.args, "--setenv", "CTX_AGENT_ROLE", "worker")
            && contains_arg_triplet(&command.args, "--setenv", "CTX_AGENT_MODEL", "main")
            && contains_arg_triplet(&command.args, "--setenv", "CTX_AGENT_LIFE", "owned")
            && contains_arg_triplet(&command.args, "--setenv", "CTX_AGENT_SUBJECT", "executor_t")
            && contains_arg_triplet(&command.args, "--setenv", "HOME", "/home/agent")
            && contains_arg_triplet(&command.args, "--setenv", "PATH", "/usr/bin:/bin")
            && contains_arg_triplet(&command.args, "--setenv", "USER", "executor")
            && contains_arg_triplet(&command.args, "--setenv", "LOGNAME", "executor")
            && contains_arg_triplet(&command.args, "--setenv", "SHELL", "/usr/bin/bash")
            && contains_arg_triplet(&command.args, "--setenv", "TERM", "xterm-256color")
            && contains_arg_triplet(&command.args, "--setenv", "LANG", "C.UTF-8")
            && contains_arg_triplet(&command.args, "--setenv", "GIT_OPTIONAL_LOCKS", "0")
            && contains_arg_triplet(
                &command.args,
                "--setenv",
                "CTX_PATH",
                "/ctx/tool:/ctx/home/1000/tool"
            )
    );
    let bwrap_index = command.args.iter().position(|arg| arg == "/usr/bin/bwrap");
    assert!(
        bwrap_index.is_some(),
        "missing bwrap in command: {command:?}"
    );
    let Some(bwrap_index) = bwrap_index else {
        return;
    };
    let bwrap_tail = command.args.get(bwrap_index + 1..).unwrap_or_default();
    assert!(
        !bwrap_tail
            .iter()
            .any(|arg| arg.starts_with("CTX_") && arg.contains('=')),
        "bwrap arguments must not contain raw KEY=value env entries: {command:?}"
    );
}

#[test]
fn launch_process_for_rejects_unavailable_user_manager() {
    let command = AgentLaunchCommand {
        program: "/usr/bin/systemd-run".to_owned(),
        args: vec!["--user".to_owned(), "/usr/bin/env".to_owned()],
    };
    let identity = AgentUnixIdentity::new(u32::MAX, u32::MAX, []);
    let result = launch_process_for(&identity, &command);
    assert!(
        matches!(result, Err(ref error) if error.kind() == std::io::ErrorKind::NotFound),
        "unavailable manager must fail closed: {result:?}"
    );
}

#[test]
fn systemctl_user_command_uses_clean_runtime_environment() {
    let command = systemctl_user_command(["stop", "cortexfs-agent-executor-test-terminal.service"]);
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
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

    assert_eq!(command.get_program(), "/usr/bin/systemctl");
    assert_eq!(
        args,
        vec![
            "--user".to_owned(),
            "stop".to_owned(),
            "cortexfs-agent-executor-test-terminal.service".to_owned()
        ]
    );
    assert_clean_user_systemd_env(&envs);
}

fn assert_clean_user_systemd_env(envs: &[(String, Option<String>)]) {
    assert!(
        envs.iter()
            .any(|entry| entry.0 == "PATH" && entry.1.as_deref() == Some("/usr/bin:/bin")),
        "missing sanitized PATH in {envs:?}"
    );
    assert!(
        envs.iter().all(|entry| matches!(
            entry.0.as_str(),
            "PATH" | "XDG_RUNTIME_DIR" | "DBUS_SESSION_BUS_ADDRESS"
        )),
        "unexpected systemd client environment in {envs:?}"
    );
}

#[test]
fn agent_start_status_lines_follow_systemctl_shape() {
    let lines = agent_start_status_lines(
        false,
        "executor",
        "main",
        "owned",
        "agent",
        &[("UID", "1000"), ("GID", "100"), ("Groups", "10 20")],
        "default",
        "cortexfs-agent-executor-default-terminal",
        Some("abc123"),
        "/workspace",
        Some("/repo"),
        Path::new("/ctx/home/1000/agent/executor/session/default/terminal/main.sock"),
        Path::new(cortexfs::runtime::terminal::broker::BROKER_SOCKET),
        "1000",
    );

    let expected = [
        "● cortexfs-agent-executor-default-terminal.service - CortexFS agent terminal",
        "     Loaded: loaded (/run/user/1000/systemd/transient/cortexfs-agent-executor-default-terminal.service; transient)",
        "     Active: active (running)",
        " Invocation: abc123",
        "      Agent: executor",
        "      Model: main",
        "       Life: owned",
        "       Role: agent",
        "     UID: 1000",
        "     GID: 100",
        "     Groups: 10 20",
        "    Session: default",
        "        CWD: /workspace",
        "  Workspace: /repo",
        "     Socket: /ctx/home/1000/agent/executor/session/default/terminal/main.sock",
        " Runtime Socket: /run/cortexfs/terminal/broker.sock",
    ];
    assert_eq!(lines, expected.map(str::to_owned));
}

#[test]
fn visible_terminal_socket_rejects_readonly_alias_path() {
    let runtime = unique_test_dir("ctx-agent-terminal-runtime-alias").join("main.sock");
    let visible = PathBuf::from(format!(
        "/usr/cortexfs-terminal-alias-test-{}/main.sock",
        std::process::id()
    ));

    assert!(ensure_best_effort_visible_terminal_socket(&visible, &runtime).is_err());
}

#[test]
fn visible_chat_socket_rejects_readonly_alias_path() {
    let runtime = unique_test_dir("ctx-agent-chat-runtime-alias").join("executor.sock");
    let visible = PathBuf::from(format!(
        "/usr/cortexfs-chat-alias-test-{}.sock",
        std::process::id()
    ));

    assert!(ensure_agent_chat_socket(&visible, &runtime).is_err());
}

#[test]
fn visible_terminal_socket_verifies_expected_alias() {
    let root = unique_test_dir("ctx-agent-terminal-visible-alias");
    let visible = root.join("session/terminal/main.sock");
    let runtime = root.join("runtime/main.sock");

    assert_eq!(
        ensure_best_effort_visible_terminal_socket(&visible, &runtime),
        Ok(true)
    );
    assert_eq!(
        ensure_best_effort_visible_terminal_socket(&visible, &runtime),
        Ok(false)
    );
    assert!(matches!(fs::read_link(visible), Ok(target) if target == runtime));
}

#[test]
fn visible_chat_socket_verifies_expected_alias() {
    let root = unique_test_dir("ctx-agent-chat-visible-alias");
    let visible = root.join("agent/executor.sock");
    let runtime = root.join("runtime/executor.sock");
    assert!(fs::create_dir_all(root.join("agent")).is_ok());

    assert!(matches!(
        ensure_agent_chat_socket(&visible, &runtime),
        Ok(AgentChatAliasState::Created)
    ));
    assert!(matches!(fs::read_link(visible), Ok(target) if target == runtime));
}

#[test]
fn agent_attach_missing_terminal_suggests_start_command() {
    let socket = unique_test_dir("agent-attach-missing-terminal").join("main.sock");
    let result = stream_terminal_socket(&socket, true, "executor", "test");
    assert!(matches!(
        result,
        Err(ref error)
            if error.message.contains("terminal is not running")
                && error.message.contains("ctx agent start executor --session test")
    ));
}

#[test]
fn agent_attach_missing_terminal_quotes_unsafe_session_in_start_hint() {
    let socket = unique_test_dir("agent-attach-missing-terminal-unsafe-session").join("main.sock");
    let result =
        stream_terminal_socket(&socket, true, "executor", "safe; touch CORTEXFS_HINT_PWNED #");
    assert!(matches!(
        result,
        Err(ref error)
            if error.message.contains("terminal is not running")
                && error.message.contains(
                    "ctx agent start executor --session 'safe; touch CORTEXFS_HINT_PWNED #'"
            )
    ));
}

#[test]
fn agent_start_chat_socket_command_uses_socket_activation() {
    let root = clean_test_dir("ctx-agent-start-chat-socket-command");
    let socket = root.join("runtime").join("executor.sock");
    let unit = agent_chat_unit(&root, "executor");
    let command = agent_chat_socket_systemd_command(&root, "executor", &socket, &unit);

    assert_eq!(command.program, "/usr/bin/systemd-run");
    assert!(command.args.contains(&"--user".to_owned()));
    assert!(contains_arg_pair(&command.args, "--unit", &unit));
    assert!(command.args.contains(&"--collect".to_owned()));
    assert!(command.args.contains(&"--property=MemoryMax=512M".to_owned()));
    assert!(command.args.contains(&"--property=CPUQuota=100%".to_owned()));
    assert!(command.args.contains(&"--property=TasksMax=128".to_owned()));
    assert!(command.args.contains(&"--property=OOMPolicy=stop".to_owned()));
    assert!(contains_arg_pair(
        &command.args,
        "--socket-property",
        &format!("ListenStream={}", socket.display())
    ));
    assert!(contains_arg_pair(
        &command.args,
        "--socket-property",
        "SocketMode=0666"
    ));
    assert!(contains_arg_pair(&command.args, "--agent", "executor"));
    assert!(
        command
            .args
            .iter()
            .any(|arg| arg.ends_with("cortexfs-agent-runtime"))
    );
}

#[test]
fn agent_start_chat_socket_path_is_root_scoped() {
    let left = clean_test_dir("ctx-agent-chat-socket-left");
    let right = clean_test_dir("ctx-agent-chat-socket-right");

    let left_socket = agent_chat_runtime_socket(&left, "executor");
    let right_socket = agent_chat_runtime_socket(&right, "executor");

    assert!(matches!(left_socket, Ok(ref socket) if socket.ends_with("executor.sock")));
    assert!(matches!((&left_socket, &right_socket), (Ok(left), Ok(right)) if left != right));
    assert_eq!(
        agent_chat_unit(&left, "executor"),
        agent_chat_unit(&left, "executor")
    );
    assert_ne!(
        agent_chat_unit(&left, "executor"),
        agent_chat_unit(&right, "executor")
    );
}

#[test]
fn agent_start_chat_unit_normalizes_existing_relative_root() {
    let current = std::env::current_dir();
    assert!(current.is_ok());
    let Ok(current) = current else { return };

    assert_eq!(
        agent_chat_unit(Path::new("."), "executor"),
        agent_chat_unit(&current, "executor")
    );
}

#[test]
fn agent_socket_path_prefers_current_user_agent_override() {
    let root = clean_test_dir("ctx-agent-user-socket-override");
    let uid = current_uid_for_test();
    let control = root.join("home").join(&uid).join("agent").join("executor.d");
    assert!(fs::create_dir_all(&control).is_ok());

    assert_eq!(
        agent_socket_path(&root, "executor"),
        Ok(root.join("home").join(uid).join("agent").join("executor.sock"))
    );
}

#[test]
fn agent_start_chat_alias_failure_rolls_back_terminal_resources_and_keeps_error() {
    let root = clean_test_dir("agent-start-chat-alias-rollback");
    let visible = root.join("visible.sock");
    let runtime = root.join("runtime.sock");
    let chat_placeholder = root.join("chat.sock");
    let chat_runtime = root.join("expected-chat.sock");
    let unrelated = root.join("unrelated.sock");
    let reused_visible = root.join("reused-visible.sock");
    let reused_runtime = root.join("reused-runtime.sock");
    let status = root.join("status");
    assert!(fs::create_dir_all(&root).is_ok());
    let terminal_listener = UnixListener::bind(&runtime);
    assert!(terminal_listener.is_ok());
    assert!(symlink(&runtime, &visible).is_ok());
    assert!(symlink(&unrelated, &chat_placeholder).is_ok());
    assert!(symlink(&reused_runtime, &reused_visible).is_ok());
    assert_eq!(
        ensure_best_effort_visible_terminal_socket(&reused_visible, &reused_runtime),
        Ok(false)
    );
    write_text_file(&status, "idle\n");
    let terminal_reset = Cell::new(false);
    let chat_reset = Cell::new(false);
    let original = CliError::unavailable("injected chat alias failure");

    let injected: Result<(), CliError> = Err(original);
    let result = match injected {
        Ok(()) => Ok(()),
        Err(error) => {
            rollback_agent_start_resources_with(
                "terminal-unit",
                None,
                &[
                    (visible.as_path(), runtime.as_path()),
                    (chat_placeholder.as_path(), chat_runtime.as_path()),
                ],
                &[runtime.as_path()],
                |_unit| terminal_reset.set(true),
                |_unit| chat_reset.set(true),
            );
            Err(error)
        }
    };

    assert_eq!(
        result,
        Err(CliError::unavailable("injected chat alias failure"))
    );
    assert!(terminal_reset.get());
    assert!(!chat_reset.get());
    assert!(fs::symlink_metadata(visible).is_err());
    assert!(fs::symlink_metadata(runtime).is_err());
    assert!(matches!(
        fs::read_link(chat_placeholder),
        Ok(target) if target == unrelated
    ));
    assert!(matches!(
        fs::read_link(reused_visible),
        Ok(target) if target == reused_runtime
    ));
    assert_eq!(fs::read_to_string(status).unwrap_or_default(), "idle\n");
    drop(terminal_listener);
}

#[test]
fn agent_chat_alias_existing_same_target_survives_rollback() {
    let root = clean_test_dir("agent-chat-alias-existing");
    assert!(fs::create_dir_all(&root).is_ok());
    let visible = root.join("visible.sock");
    let runtime = root.join("runtime.sock");
    assert!(symlink(&runtime, &visible).is_ok());
    let terminal_reset = Cell::new(false);
    let chat_reset = Cell::new(false);

    let state = ensure_agent_chat_socket(&visible, &runtime);
    assert!(matches!(state, Ok(AgentChatAliasState::ExistingSameTarget)));
    let Ok(state) = state else { return };
    rollback_agent_late_chat_start_with(
        ("terminal-unit", "chat-unit"),
        (&[], &[]),
        (&visible, &runtime, &state),
        |_unit| terminal_reset.set(true),
        |_unit| chat_reset.set(true),
    );
    assert!(terminal_reset.get());
    assert!(chat_reset.get());
    assert!(matches!(fs::read_link(visible), Ok(target) if target == runtime));
}

#[test]
fn agent_chat_alias_created_by_start_is_removed_by_rollback() {
    let root = clean_test_dir("agent-chat-alias-created");
    assert!(fs::create_dir_all(&root).is_ok());
    let visible = root.join("visible.sock");
    let runtime = root.join("runtime.sock");

    let state = ensure_agent_chat_socket(&visible, &runtime);
    assert!(matches!(state, Ok(AgentChatAliasState::Created)));
    assert!(matches!(fs::read_link(&visible), Ok(target) if target == runtime));
    let Ok(state) = state else { return };
    rollback_agent_late_chat_start_with(
        ("terminal-unit", "chat-unit"),
        (&[], &[]),
        (&visible, &runtime, &state),
        |_unit| {},
        |_unit| {},
    );
    assert!(fs::symlink_metadata(visible).is_err());
}

#[test]
fn agent_chat_placeholder_metadata_is_restored_by_rollback() {
    let root = clean_test_dir("agent-chat-alias-placeholder");
    assert!(fs::create_dir_all(&root).is_ok());
    let visible = root.join("visible.sock");
    let runtime = root.join("runtime.sock");
    let listener = UnixListener::bind(&visible);
    assert!(listener.is_ok());
    assert!(fs::set_permissions(&visible, fs::Permissions::from_mode(0o750)).is_ok());
    let before = fs::symlink_metadata(&visible)
        .map(|metadata| {
            (
                metadata.permissions().mode() & 0o7777,
                metadata.uid(),
                metadata.gid(),
            )
        })
        .ok();

    let state = ensure_agent_chat_socket(&visible, &runtime);
    assert!(matches!(
        state,
        Ok(AgentChatAliasState::ReplacedPlaceholder { mode, uid, gid })
            if Some((mode, uid, gid)) == before
    ));
    let Ok(state) = state else { return };
    drop(listener);
    rollback_agent_late_chat_start_with(
        ("terminal-unit", "chat-unit"),
        (&[], &[]),
        (&visible, &runtime, &state),
        |_unit| {},
        |_unit| {},
    );
    assert!(matches!(
        fs::symlink_metadata(visible),
        Ok(metadata)
            if metadata.file_type().is_socket()
                && Some((
                    metadata.permissions().mode() & 0o7777,
                    metadata.uid(),
                    metadata.gid(),
                )) == before
    ));
}

#[test]
fn agent_chat_alias_rejects_mismatch_and_regular_file_without_changes() {
    let root = clean_test_dir("agent-chat-alias-refuse");
    assert!(fs::create_dir_all(&root).is_ok());
    let visible = root.join("visible.sock");
    let runtime = root.join("runtime.sock");
    let other = root.join("other.sock");
    assert!(symlink(&other, &visible).is_ok());

    assert!(ensure_agent_chat_socket(&visible, &runtime).is_err());
    assert!(matches!(fs::read_link(&visible), Ok(target) if target == other));

    assert!(fs::remove_file(&visible).is_ok());
    write_text_file(&visible, "keep\n");
    assert!(ensure_agent_chat_socket(&visible, &runtime).is_err());
    assert_eq!(fs::read_to_string(visible).unwrap_or_default(), "keep\n");
}

#[test]
fn exact_socket_alias_cleanup_restores_mismatched_alias_after_claim() {
    let root = clean_test_dir("agent-socket-claim-mismatch");
    assert!(fs::create_dir_all(&root).is_ok());
    let visible = root.join("visible.sock");
    let expected = root.join("expected.sock");
    let other = root.join("other.sock");
    assert!(symlink(&other, &visible).is_ok());

    let result = cortexfs::agent::launch::remove_exact_socket_alias(&visible, &expected);

    assert!(matches!(
        result,
        Err(ref error) if error.kind() == std::io::ErrorKind::AlreadyExists
    ));
    assert!(matches!(fs::read_link(&visible), Ok(target) if target == other));
    assert!(fs::read_dir(&root).is_ok_and(|entries| {
        entries
            .flatten()
            .all(|entry| !entry.file_name().to_string_lossy().contains(".claim-"))
    }));
}
#[test]
fn system_agent_backing_socket_matches_host_backing_path() {
    let root = Path::new("/var/lib/cortexfs/storage/current");
    assert_eq!(
        cortexfs_paths::agent_backing_socket(root, "child"),
        root.join("agent/child.sock")
    );
}
