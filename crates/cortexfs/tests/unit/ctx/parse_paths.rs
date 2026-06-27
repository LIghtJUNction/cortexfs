use std::fmt::Write;

fn parse_cli_args(args: &[&str]) -> Result<Cli, CliError> {
    parse(args.iter().map(std::ffi::OsString::from).collect())
}

#[test]
fn parses_leading_root_global_option_only() {
    let cli = parse_cli_args(&["--root", "/tmp/ctx-alt", "status"]);
    assert!(matches!(
        cli,
        Ok(Cli {
            ref root,
            command: Command::Status,
        }) if root == Path::new("/tmp/ctx-alt")
    ));

    let exec = parse_cli_args(&["exec", "agent/coder", "--root", "/tmp/ctx-alt"]);
    assert!(matches!(
        exec,
        Ok(Cli {
            command: Command::Exec { ref path, ref args },
            ..
        }) if path == "agent/coder"
            && args == &["--root".to_owned(), "/tmp/ctx-alt".to_owned()]
    ));

    let tool = parse_cli_args(&["tool", "tsh.config", "--root"]);
    assert!(matches!(
        tool,
        Ok(Cli {
            command: Command::Tool { ref name, ref args },
            ..
        }) if name == "tsh.config" && args == &["--root".to_owned()]
    ));
}

#[test]
fn zero_arg_commands_reject_extra_arguments() {
    let status = cmd!("status", "extra");
    assert!(matches!(
        status,
        Err(ref error) if error.code == 2 && error.message == "unexpected argument: extra"
    ));

    let abi = cmd!("abi", "extra");
    assert!(matches!(
        abi,
        Err(ref error) if error.code == 2 && error.message == "unexpected argument: extra"
    ));
}

#[test]
fn parses_spec_which_command() {
    let command = cmd!("which", "tool", "fs.read");
    assert!(matches!(
        command,
        Ok(Command::Which(ObjectClass::Tool, ref name)) if name == "fs.read"
    ));
}

#[test]
fn parses_man_command() {
    let index = cmd!("man");
    assert!(matches!(index, Ok(Command::Man { topic: None })));

    let agent = cmd!("man", "agent");
    assert!(matches!(
        agent,
        Ok(Command::Man { topic: Some(ref topic) }) if topic == "agent"
    ));

    let extra = cmd!("man", "agent", "extra");
    assert!(matches!(
        extra,
        Err(ref error) if error.code == 2 && error.message == "unexpected argument: extra"
    ));
}

#[test]
fn parses_top_level_file_content_commands() {
    let cat = cmd!("cat", "agent/coder.d/cwd");
    assert!(matches!(
        cat,
        Ok(Command::Cat { ref path }) if path == "agent/coder.d/cwd"
    ));

    let set = cmd!("set", "agent/coder.d/cwd", "/work");
    assert!(matches!(
        set,
        Ok(Command::Set { ref path, ref value }) if path == "agent/coder.d/cwd" && value == "/work"
    ));

    let append = cmd!("append", "agent/coder.d/path", "/ctx/tool");
    assert!(matches!(
        append,
        Ok(Command::Append { ref path, ref value })
            if path == "agent/coder.d/path" && value == "/ctx/tool"
    ));
}

#[test]
fn parses_file_metadata_command() {
    let explicit = cmd!("file", "type", "tool/fs.read");
    assert!(matches!(
        explicit,
        Ok(Command::File(ref args))
            if args.command == FileCommand::Type
                && args.path == "tool/fs.read"
    ));

    let shorthand = cmd!("file", "tool/fs.read");
    assert!(matches!(
        shorthand,
        Ok(Command::File(ref args))
            if args.command == FileCommand::Info
                && args.path == "tool/fs.read"
    ));
}

#[test]
fn parses_ls_path_command() {
    let root = cmd!("ls");
    assert!(matches!(root, Ok(Command::Ls(LsTarget::Root))));

    let home = cmd!("ls", "home");
    assert!(matches!(
        home,
        Ok(Command::Ls(LsTarget::Path(ref path))) if path == "home"
    ));

    let tool = cmd!("ls", "tool");
    assert!(matches!(
        tool,
        Ok(Command::Ls(LsTarget::Path(ref path))) if path == "tool"
    ));
}

#[test]
fn parses_session_file_commands() {
    let history = cmd!("history", "coder");
    assert!(matches!(
        history,
        Ok(Command::History {
            ref agent,
            session: None
        }) if agent == "coder"
    ));

    let history_flagged = cmd!("history", "coder", "--session", "focus");
    assert!(matches!(
        history_flagged,
        Ok(Command::History {
            ref agent,
            session: Some(ref session)
        }) if agent == "coder" && session == "focus"
    ));

    let output = cmd!("agent", "output", "coder", "--session", "default");
    assert!(matches!(
        output,
        Ok(Command::Agent(AgentArgs::Output {
            ref name,
            session: Some(ref session)
        })) if name == "coder" && session == "default"
    ));

    let prompt = cmd!("agent", "prompt", "coder");
    assert!(matches!(
        prompt,
        Ok(Command::Agent(AgentArgs::Prompt { ref name })) if name == "coder"
    ));

    let resume = cmd!("resume", "coder", "default");
    assert!(matches!(
        resume,
        Ok(Command::Resume {
            ref agent,
            session: Some(ref session)
        }) if agent == "coder" && session == "default"
    ));

    let resume_flagged = cmd!("resume", "coder", "-s", "focus");
    assert!(matches!(
        resume_flagged,
        Ok(Command::Resume {
            ref agent,
            session: Some(ref session)
        }) if agent == "coder" && session == "focus"
    ));

    let send = cmd!("send", "coder", "default", "hello");
    assert!(matches!(
        send,
        Ok(Command::Send {
            ref agent,
            session: Some(ref session),
            ref input
        }) if agent == "coder" && session == "default" && input == "hello"
    ));

    let send_current = cmd!("send", "coder", "hello");
    assert!(matches!(
        send_current,
        Ok(Command::Send {
            ref agent,
            session: None,
            ref input
        }) if agent == "coder" && input == "hello"
    ));

    let send_flagged = cmd!("send", "coder", "--session", "focus", "hello", "world");
    assert!(matches!(
        send_flagged,
        Ok(Command::Send {
            ref agent,
            session: Some(ref session),
            ref input
        }) if agent == "coder" && session == "focus" && input == "hello world"
    ));

    let ping = cmd!("ping", "agent/coder");
    assert!(matches!(
        ping,
        Ok(Command::Ping { ref path }) if path == "agent/coder"
    ));

    let cancel = cmd!("cancel", "agent/coder", "run-1");
    assert!(matches!(
        cancel,
        Ok(Command::Cancel { ref path, ref run }) if path == "agent/coder" && run == "run-1"
    ));
}

#[test]
fn parses_subcommand_help_before_required_args() {
    let latest = cmd!("latest", "--help");
    assert!(matches!(
        latest,
        Err(ref error) if error.code == 2 && error.message == "unknown command: latest"
    ));

    let send = cmd!("send", "--help");
    assert!(matches!(
        send,
        Ok(Command::HelpTopic(ref topic)) if topic == "send"
    ));

    let resume = cmd!("resume", "--help");
    assert!(matches!(
        resume,
        Ok(Command::HelpTopic(ref topic)) if topic == "resume"
    ));

    let agent = cmd!("agent", "--help");
    assert!(matches!(
        agent,
        Ok(Command::HelpTopic(ref topic)) if topic == "agent"
    ));

    let agent_watch = cmd!("agent", "watch", "--help");
    assert!(matches!(
        agent_watch,
        Ok(Command::HelpTopic(ref topic)) if topic == "agent watch"
    ));
}


#[test]
fn parses_literal_help_as_positional_argument() {
    let which_tool = cmd!("which-tool", "help");
    assert!(matches!(
        which_tool,
        Ok(Command::Which(ObjectClass::Tool, ref name)) if name == "help"
    ));

    let validate_name = cmd!("validate-name", "help");
    assert!(matches!(
        validate_name,
        Ok(Command::ValidateName(ref name)) if name == "help"
    ));

    let mount = cmd!("mount", "help");
    assert!(matches!(
        mount,
        Ok(Command::Mount { source: None, mountpoint: Some(ref mountpoint) })
            if mountpoint == &PathBuf::from("help")
    ));

    let agent_stop = cmd!("agent", "stop", "help");
    assert!(matches!(
        agent_stop,
        Ok(Command::Agent(AgentArgs::Stop { ref name })) if name == "help"
    ));

    let agent_status = cmd!("agent", "status", "help");
    assert!(matches!(
        agent_status,
        Ok(Command::Agent(AgentArgs::Status { ref name })) if name == "help"
    ));

    let agent_new = cmd!("agent", "new", "help");
    assert!(matches!(
        agent_new,
        Ok(Command::Agent(AgentArgs::New(ref args))) if args.name == "help"
    ));
}

#[test]
fn parses_agent_session_client_commands() {
    let send = cmd!("agent", "send", "coder", "--session", "test", "hello", "world");
    assert!(matches!(
        send,
        Ok(Command::Agent(AgentArgs::Send {
            ref name,
            session: Some(ref session),
            ref input,
            raw: false,
        })) if name == "coder" && session == "test" && input == "hello world"
    ));

    let repl = cmd!("agent", "repl", "coder", "--raw");
    assert!(matches!(
        repl,
        Ok(Command::Agent(AgentArgs::Repl {
            ref name,
            session: None,
            raw: true,
        })) if name == "coder"
    ));

    let cancel = cmd!("agent", "cancel", "coder", "--session", "test", "run-1");
    assert!(matches!(
        cancel,
        Ok(Command::Agent(AgentArgs::Cancel {
            ref name,
            session: Some(ref session),
            run: Some(ref run),
            raw: false,
        })) if name == "coder" && session == "test" && run == "run-1"
    ));
}

#[test]
fn rejects_removed_agent_sh_command() {
    let command = cmd!("agent-sh", "--session", "focus", "coder");
    assert!(matches!(
        command,
        Err(ref error) if error.code == 2 && error.message == "unknown command: agent-sh"
    ));
}

#[test]
fn object_execution_command_uses_clean_runtime_environment() {
    let root = Path::new("/tmp/cortexfs-clean-exec-root");
    let command = object_execution_command(root, &root.join("tool").join("example"));
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
fn agent_ps_reads_parent_status_and_pid_controls() {
    let root = clean_test_dir("ctx-agent-ps");
    create_agent_fixture(&root, "coder", "", "idle", "");
    create_agent_fixture(&root, "reviewer", "agent:coder session:default run:r1", "busy", "123");
    create_agent_fixture(&root, "auditor", "agent:reviewer", "ready", "");

    let processes = read_agent_processes(&root);
    assert!(processes.is_ok());
    let mut processes = processes.unwrap_or_default();
    processes.sort_by(|left, right| left.name.cmp(&right.name));
    assert!(processes.iter().any(|process| {
        process.name == "reviewer"
            && process.parent.as_deref() == Some("coder")
            && process.status == "busy"
            && process.pid.as_deref() == Some("123")
    }));

    let root_process = processes
        .iter()
        .find(|process| process.name == "coder")
        .cloned();
    assert!(root_process.is_some());
    let Some(root_process) = root_process else {
        return;
    };
    let mut rendered = Vec::new();
    render_agent_process_tree(&root_process, &processes, "", true, true, &mut rendered);
    assert_eq!(
        rendered,
        vec![
            "coder [idle]".to_owned(),
            "`- reviewer [busy] pid=123".to_owned(),
            "   `- auditor [ready]".to_owned(),
        ]
    );
}

#[test]
fn agent_process_tree_escapes_control_file_values() {
    let process = AgentProcess {
        name: "coder".to_owned(),
        parent: None,
        status: "idle\u{1b}]52;c;payload\u{7}".to_owned(),
        pid: Some("123\u{1b}[31m".to_owned()),
    };
    let mut rendered = Vec::new();

    render_agent_process_tree(&process, std::slice::from_ref(&process), "", true, true, &mut rendered);

    assert_eq!(
        rendered,
        vec!["coder [idle\\u{1b}]52;c;payload\\u{7}] pid=123\\u{1b}[31m".to_owned()]
    );
    let line = rendered.first().map_or("", String::as_str);
    assert!(!line.as_bytes().contains(&0x1b));
    assert!(!line.as_bytes().contains(&0x07));
}

#[test]
fn status_helpers_report_ctx_and_agent_tree() {
    let root = clean_test_dir("ctx-status-tree");
    write_text_file(&root.join("status"), "ready\n");
    create_agent_fixture(&root, "coder", "", "idle", "");
    create_agent_fixture(&root, "reviewer", "agent:coder session:default run:r1", "busy", "123");

    assert_eq!(ctx_state(true, true, true), "running");
    assert_eq!(ctx_state(true, true, false), "available");
    assert_eq!(read_ctx_status(&root), "ready");

    let processes = read_status_agent_processes(&root);
    assert!(processes.is_ok());
    let rendered = render_agent_status_lines(&processes.unwrap_or_default());
    assert_eq!(
        rendered,
        vec![
            "coder [idle]".to_owned(),
            "`- reviewer [busy] pid=123".to_owned(),
        ]
    );
}

#[test]
fn ctx_root_shape_does_not_follow_symlink_root() {
    let root = clean_test_dir("ctx-status-root-shape");
    let outside = clean_test_dir("ctx-status-root-shape-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    let link = root.join("ctx");
    assert!(std::os::unix::fs::symlink(&outside, &link).is_ok());

    assert_eq!(ctx_root_shape(&outside), (true, true));
    assert_eq!(ctx_root_shape(&link), (true, false));
    assert_eq!(ctx_state(true, false, false), "invalid");
}

#[test]
fn ctx_root_entry_present_does_not_follow_symlink_entry() {
    let root = clean_test_dir("ctx-status-root-entry-shape");
    let outside = clean_test_dir("ctx-status-root-entry-shape-outside");
    assert!(fs::create_dir_all(root.join("status")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("bin")).is_ok());

    assert!(ctx_root_entry_present(&root, "status"));
    assert!(!ctx_root_entry_present(&root, "bin"));
}

#[test]
fn status_tolerates_missing_agent_directory() {
    let root = clean_test_dir("ctx-status-no-agent");
    write_text_file(&root.join("status"), "ready\n");

    let processes = read_status_agent_processes(&root);
    assert_eq!(processes, Ok(Vec::new()));
}

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
fn agent_terminal_socket_uses_session_terminal_main_socket() {
    let root = clean_test_dir("ctx-agent-terminal-socket");
    let socket = agent_terminal_socket(&root, "coder", "test");
    assert_eq!(
        socket,
        Ok(root
            .join("home")
            .join(current_uid_for_test())
            .join("agent")
            .join("coder")
            .join("session")
            .join("test")
            .join("terminal")
            .join("main.sock"))
    );
}

#[test]
fn agent_start_builds_sandboxed_terminal_command() {
    let root = clean_test_dir("ctx-agent-start-bwrap-view");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let view = derive_agent_runtime_view(&root, "coder");
    assert!(view.is_ok(), "reference coder view: {view:?}");
    let Ok(view) = view else {
        return;
    };
    let args = AgentStartArgs {
        name: "coder".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: vec![AgentMount {
            source: "/repo".to_owned(),
            target: "/workspace".to_owned(),
            mode: "rw".to_owned(),
        }],
    };
    let socket = PathBuf::from("/ctx/home/1000/agent/coder/session/test/terminal/main.sock");
    let home = PathBuf::from("/ctx/home/1000");
    let cli_mounts = vec![AgentMount {
        source: "/repo".to_owned(),
        target: "/workspace".to_owned(),
        mode: "rw".to_owned(),
    }];
    let bwrap = agent_bwrap_args(&root, &args, &cli_mounts, &view, &socket, &home);
    assert!(contains_arg_triplet(&bwrap, "--ro-bind", "/ctx", "/ctx"));
    assert!(contains_arg_triplet(
        &bwrap,
        "--bind",
        "/ctx/home/1000/agent/coder",
        "/home/agent"
    ));
    assert!(contains_arg_triplet(&bwrap, "--bind", "/repo", "/workspace"));
    assert!(bwrap.contains(&"--unshare-net".to_owned()));
    assert!(contains_arg_pair(&bwrap, "--dir", "/home"));
    assert!(contains_ro_bind_stub(&bwrap, "/etc/profile"));
    assert!(contains_ro_bind_stub(&bwrap, "/etc/bash.bashrc"));
    assert!(contains_arg_pair(&bwrap, "--tmpfs", "/etc/profile.d"));
    assert!(contains_arg_pair(&bwrap, "--chdir", "/workspace"));
    assert!(contains_arg_pair(&bwrap, "--listen", socket.to_str().unwrap_or_default()));
    assert_eq!(bwrap.last().map(String::as_str), Some("/ctx/bin/tsh"));
}

#[test]
fn agent_start_default_workspace_remounts_git_read_only() {
    let source = clean_test_dir("ctx-agent-start-git-ro");
    assert!(fs::create_dir_all(source.join(".git")).is_ok());
    let args = AgentStartArgs {
        name: "coder".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: Vec::new(),
    };

    let mounts = agent_start_mounts_with_default_source(&args, &source);
    assert_eq!(
        mounts,
        vec![
            AgentMount {
                source: source.display().to_string(),
                target: "/workspace".to_owned(),
                mode: "rw".to_owned(),
            },
            AgentMount {
                source: source.join(".git").display().to_string(),
                target: "/workspace/.git".to_owned(),
                mode: "ro".to_owned(),
            },
        ]
    );

    let root = clean_test_dir("ctx-agent-start-git-bwrap-view");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let view = derive_agent_runtime_view(&root, "coder");
    assert!(view.is_ok(), "reference coder view: {view:?}");
    let Ok(view) = view else {
        return;
    };
    let socket = PathBuf::from("/ctx/home/1000/agent/coder/session/test/terminal/main.sock");
    let home = PathBuf::from("/ctx/home/1000");
    let cli_mounts = mounts;
    let bwrap = agent_bwrap_args(&root, &args, &cli_mounts, &view, &socket, &home);
    assert!(contains_arg_triplet(
        &bwrap,
        "--ro-bind",
        source.join(".git").to_str().unwrap_or_default(),
        "/workspace/.git"
    ));
}

#[test]
fn agent_start_maps_host_cwd_to_sandbox_mount_target() {
    let source = clean_test_dir("ctx-agent-start-host-cwd");
    let subdir = source.join("nested");
    assert!(fs::create_dir_all(&subdir).is_ok());
    let args = AgentStartArgs {
        name: "coder".to_owned(),
        session: "test".to_owned(),
        cwd: subdir.display().to_string(),
        default_workspace: true,
        mounts: Vec::new(),
    };
    let mounts = agent_start_mounts_with_default_source(&args, &source);

    assert_eq!(agent_start_sandbox_cwd(&args, &mounts), "/workspace/nested");
}

#[test]
fn agent_start_default_workspace_does_not_remount_symlinked_git() {
    let source = clean_test_dir("ctx-agent-start-git-symlink");
    let target = clean_test_dir("ctx-agent-start-git-symlink-target");
    assert!(fs::create_dir_all(&source).is_ok());
    assert!(fs::create_dir_all(&target).is_ok());
    assert!(std::os::unix::fs::symlink(&target, source.join(".git")).is_ok());
    let args = AgentStartArgs {
        name: "coder".to_owned(),
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
        name: "coder".to_owned(),
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
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let view = derive_agent_runtime_view(&root, "coder");
    assert!(view.is_ok(), "reference coder view: {view:?}");
    let Ok(view) = view else {
        return;
    };
    let args = AgentStartArgs {
        name: "coder".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: Vec::new(),
    };
    let socket = PathBuf::from("/ctx/home/1000/agent/coder/session/test/terminal/main.sock");
    let cli_mounts = agent_start_mounts_with_default_source(&args, Path::new("/repo"));
    let command = agent_start_systemd_command(
        &root,
        &args,
        &cli_mounts,
        &view,
        &socket,
        "cortexfs-agent-coder-test-terminal",
    );
    assert!(
        command.program == "/usr/bin/systemd-run"
                && command.args.contains(&"--user".to_owned())
                && contains_arg_pair(&command.args, "--property", "Restart=always")
                && contains_arg_pair(&command.args, "--property", "RestartSec=250ms")
                && command.args.contains(&"-i".to_owned())
                && command.args.contains(&"PATH=/usr/bin:/bin".to_owned())
                && command.args.contains(&"/usr/bin/bwrap".to_owned())
                && contains_arg_pair(&command.args, "--clearenv", "--setenv")
                && contains_arg_triplet(&command.args, "--setenv", "CTX_ROOT", &root.display().to_string())
                && contains_arg_triplet(&command.args, "--setenv", "CTX_HOME", &root.join("home").join("1000").display().to_string())
                && contains_arg_triplet(&command.args, "--setenv", "CTX_AGENT", "coder")
                && contains_arg_triplet(&command.args, "--setenv", "CTX_AGENT_SUBJECT", "coder_t")
                && contains_arg_triplet(&command.args, "--setenv", "HOME", "/home/agent")
                && contains_arg_triplet(&command.args, "--setenv", "PATH", "/usr/bin:/bin")
                && contains_arg_triplet(&command.args, "--setenv", "USER", "coder")
                && contains_arg_triplet(&command.args, "--setenv", "LOGNAME", "coder")
                && contains_arg_triplet(&command.args, "--setenv", "SHELL", "/usr/bin/bash")
                && contains_arg_triplet(&command.args, "--setenv", "TERM", "xterm-256color")
                && contains_arg_triplet(&command.args, "--setenv", "LANG", "C.UTF-8")
                && contains_arg_triplet(&command.args, "--setenv", "CTX_PATH", "/ctx/tool:/ctx/home/1000/tool")
    );
    let bwrap_index = command.args.iter().position(|arg| arg == "/usr/bin/bwrap");
    assert!(bwrap_index.is_some(), "missing bwrap in command: {command:?}");
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
fn agent_start_process_command_uses_clean_runtime_environment() {
    let command = AgentStartCommand {
        program: "/usr/bin/systemd-run".to_owned(),
        args: vec!["--user".to_owned(), "/usr/bin/env".to_owned()],
    };
    let process = agent_start_process_command(&command);
    let mut envs = process
        .get_envs()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect::<Vec<_>>();
    envs.sort();

    assert_clean_user_systemd_env(&envs);
}

#[test]
fn systemctl_user_command_uses_clean_runtime_environment() {
    let command = systemctl_user_command(["stop", "cortexfs-agent-coder-test-terminal.service"]);
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
            "cortexfs-agent-coder-test-terminal.service".to_owned()
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
        "coder",
        "default",
        "cortexfs-agent-coder-default-terminal",
        Some("abc123"),
        "/workspace",
        Path::new("/ctx/home/1000/agent/coder/session/default/terminal/main.sock"),
        Path::new("/run/user/1000/cortexfs/terminal/coder/default/main.sock"),
        "1000",
    );

    assert_eq!(
        lines,
        vec![
            "● cortexfs-agent-coder-default-terminal.service - CortexFS agent terminal",
            "     Loaded: loaded (/run/user/1000/systemd/transient/cortexfs-agent-coder-default-terminal.service; transient)",
            "     Active: active (running)",
            " Invocation: abc123",
            "      Agent: coder",
            "    Session: default",
            "        CWD: /workspace",
            "     Socket: /ctx/home/1000/agent/coder/session/default/terminal/main.sock",
            " Runtime Socket: /run/user/1000/cortexfs/terminal/coder/default/main.sock",
        ]
    );
}

#[test]
fn visible_terminal_socket_treats_readonly_fuse_errors_as_best_effort() {
    assert!(visible_terminal_write_error_is_best_effort(
        &std::io::Error::from_raw_os_error(nix::libc::ENOSYS)
    ));
    assert!(visible_terminal_write_error_is_best_effort(
        &std::io::Error::from_raw_os_error(nix::libc::EROFS)
    ));
    assert!(visible_terminal_errno_is_best_effort(
        nix::errno::Errno::ENOSYS
    ));
    assert!(visible_terminal_errno_is_best_effort(
        nix::errno::Errno::EROFS
    ));
}

#[test]
fn agent_attach_missing_terminal_suggests_start_command() {
    let socket = unique_test_dir("agent-attach-missing-terminal").join("main.sock");
    let result = stream_terminal_socket(&socket, true, "coder", "test");
    assert!(matches!(
        result,
        Err(ref error)
            if error.message.contains("terminal is not running")
                && error.message.contains("ctx agent start coder --session test")
    ));
}

#[test]
fn agent_attach_missing_terminal_quotes_unsafe_session_in_start_hint() {
    let socket = unique_test_dir("agent-attach-missing-terminal-unsafe-session").join("main.sock");
    let result = stream_terminal_socket(&socket, true, "coder", "safe; touch CORTEXFS_HINT_PWNED #");
    assert!(matches!(
        result,
        Err(ref error)
            if error.message.contains("terminal is not running")
                && error.message.contains(
                    "ctx agent start coder --session 'safe; touch CORTEXFS_HINT_PWNED #'"
                )
    ));
}

#[test]
fn terminal_connect_error_classifies_socket_failures() {
    let socket = Path::new("/tmp/cortexfs-terminal.sock");
    let missing = terminal_connect_cli_error(
        socket,
        "coder",
        "test",
        &std::io::Error::from(std::io::ErrorKind::NotFound),
    );
    assert!(missing.message.contains("terminal is not running"));
    assert!(missing.message.contains("ctx agent start coder --session test"));

    let refused = terminal_connect_cli_error(
        socket,
        "coder",
        "test",
        &std::io::Error::from(std::io::ErrorKind::ConnectionRefused),
    );
    assert!(refused
        .message
        .contains("terminal socket exists but has no listener"));
    assert!(!refused.message.contains("terminal is not running"));
}

#[test]
fn agent_repl_editor_enables_terminal_signals() {
    assert!(agent_repl_editor_config().enable_signals());
}

#[test]
fn agent_repl_prompt_and_model_summary_are_chat_oriented() {
    let root = clean_test_dir("ctx-agent-repl-model-summary");
    assert!(fs::create_dir_all(root.join("agent/coder.d")).is_ok());
    assert!(fs::create_dir_all(root.join("model")).is_ok());
    assert!(fs::write(root.join("agent/coder.d/model"), "main\n").is_ok());
    assert!(
        std::os::unix::fs::symlink("/ctx/model/localhost/gpt-5.4-mini", root.join("model/main"))
            .is_ok()
    );

    assert_eq!(agent_repl_prompt(false, "coder", "default"), "coder/default ❯ ");
    assert_eq!(
        agent_repl_model_summary(false, &root, "coder"),
        "main -> /ctx/model/localhost/gpt-5.4-mini (missing)"
    );
    assert!(AGENT_REPL_COMMANDS.contains("/clear"));
}

#[test]
fn agent_repl_model_summary_rejects_symlink_model_directory() {
    let root = clean_test_dir("ctx-agent-repl-model-summary-symlink-model");
    let outside = clean_test_dir("ctx-agent-repl-model-summary-symlink-model-outside");
    assert!(fs::create_dir_all(root.join("agent/coder.d")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(fs::write(root.join("agent/coder.d/model"), "main\n").is_ok());
    assert!(
        std::os::unix::fs::symlink("/ctx/model/localhost/gpt-5.4-mini", outside.join("main"))
            .is_ok()
    );
    assert!(std::os::unix::fs::symlink(&outside, root.join("model")).is_ok());

    assert_eq!(
        agent_repl_model_summary(false, &root, "coder"),
        "main (missing alias)"
    );
}

#[test]
fn agent_repl_model_summary_does_not_follow_symlink_alias_target() {
    let root = clean_test_dir("ctx-agent-repl-model-summary-symlink-target");
    let outside = clean_test_dir("ctx-agent-repl-model-summary-symlink-target-outside");
    assert!(fs::create_dir_all(root.join("agent/coder.d")).is_ok());
    assert!(fs::create_dir_all(root.join("model/localhost")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(fs::write(root.join("agent/coder.d/model"), "main\n").is_ok());
    assert!(
        std::os::unix::fs::symlink("/ctx/model/localhost/gpt-5.4-mini", root.join("model/main"))
            .is_ok()
    );
    assert!(std::os::unix::fs::symlink(
        &outside,
        root.join("model/localhost/gpt-5.4-mini")
    )
    .is_ok());

    assert_eq!(
        agent_repl_model_summary(false, &root, "coder"),
        "main -> /ctx/model/localhost/gpt-5.4-mini (missing)"
    );
}

#[test]
fn agent_repl_exits_on_interrupt_signal_errors() {
    assert!(agent_repl_should_exit_on_readline_error(
        &rustyline::error::ReadlineError::Interrupted
    ));
    assert!(agent_repl_should_exit_on_readline_error(
        &rustyline::error::ReadlineError::Signal(rustyline::error::Signal::Interrupt)
    ));
    assert!(agent_repl_should_exit_on_readline_error(
        &rustyline::error::ReadlineError::Eof
    ));
}

#[test]
fn top_level_send_uses_agent_send_request_shape() {
    let root = clean_test_dir("ctx-top-level-send-agent-shape");
    let agent_dir = root.join("agent").join("coder.d");
    assert!(fs::create_dir_all(&agent_dir).is_ok());
    assert!(fs::write(agent_dir.join("cwd"), "/workspace\n").is_ok());
    let server = spawn_agent_socket_request_capture(&root, "coder");

    let result = run(vec![
        std::ffi::OsString::from("--root"),
        root.as_os_str().to_os_string(),
        std::ffi::OsString::from("send"),
        std::ffi::OsString::from("coder"),
        std::ffi::OsString::from("hello"),
    ]);

    assert!(matches!(result, Ok(code) if code == std::process::ExitCode::SUCCESS));
    let request = server.join();
    assert!(request.is_ok());
    let Ok(request) = request else {
        return;
    };
    assert!(request.contains("\"op\":\"send\""));
    assert!(request.contains("\"session\":\"default\""));
    assert!(request.contains("\"scope\":\"private\""));
    assert!(request.contains("\"cwd\":\"/workspace\""));
    assert!(request.contains("\"input\":\"hello\""));
}

#[test]
fn top_level_send_defaults_cwd_to_workspace() {
    let root = clean_test_dir("ctx-top-level-send-default-cwd");
    assert!(fs::create_dir_all(root.join("agent")).is_ok());
    let server = spawn_agent_socket_request_capture(&root, "coder");

    let result = run(vec![
        std::ffi::OsString::from("--root"),
        root.as_os_str().to_os_string(),
        std::ffi::OsString::from("send"),
        std::ffi::OsString::from("coder"),
        std::ffi::OsString::from("hello"),
    ]);

    assert!(matches!(result, Ok(code) if code == std::process::ExitCode::SUCCESS));
    let request = server.join();
    assert!(request.is_ok());
    let Ok(request) = request else {
        return;
    };
    assert!(request.contains("\"cwd\":\"/workspace\""));
}

#[test]
fn top_level_resume_uses_agent_resume_request_shape() {
    let root = clean_test_dir("ctx-top-level-resume-agent-shape");
    assert!(fs::create_dir_all(root.join("agent")).is_ok());
    let server = spawn_agent_socket_request_capture(&root, "coder");

    let result = run(vec![
        std::ffi::OsString::from("--root"),
        root.as_os_str().to_os_string(),
        std::ffi::OsString::from("resume"),
        std::ffi::OsString::from("coder"),
    ]);

    assert!(matches!(result, Ok(code) if code == std::process::ExitCode::SUCCESS));
    let request = server.join();
    assert!(request.is_ok());
    let Ok(request) = request else {
        return;
    };
    assert!(request.contains("\"op\":\"resume\""));
    assert!(request.contains("\"session\":\"default\""));
    assert!(!request.contains("\"scope\""));
    assert!(!request.contains("\"input\""));
}

fn spawn_agent_socket_request_capture(
    root: &Path,
    agent: &str,
) -> std::thread::JoinHandle<String> {
    let socket = root.join("agent").join(format!("{agent}.sock"));
    let listener = std::os::unix::net::UnixListener::bind(&socket);
    assert!(listener.is_ok());
    let Ok(listener) = listener else {
        return std::thread::spawn(String::new);
    };

    std::thread::spawn(move || {
        let accepted = listener.accept();
        assert!(accepted.is_ok());
        let Ok((mut stream, _addr)) = accepted else {
            return String::new();
        };
        let mut request = String::new();
        assert!(std::io::Read::read_to_string(&mut stream, &mut request).is_ok());
        assert!(
            std::io::Write::write_all(&mut stream, b"{\"type\":\"done\"}\n").is_ok()
        );
        request
    })
}

#[test]
fn buffered_agent_renderer_keeps_assistant_output_atomic() {
    let input = concat!(
        "{\"type\":\"delta\",\"text\":\"\\u4f60\"}\n",
        "{\"type\":\"tool_call\",\"name\":\"tsh\"}\n",
        "{\"type\":\"message\",\"role\":\"tool\",\"name\":\"tsh\",\"content\":[{\"type\":\"tool_result\",\"content\":\"abc\"}]}\n",
        "{\"type\":\"delta\",\"text\":\"\\u597d\"}\n",
        "{\"type\":\"done\"}\n",
        "{\"type\":\"error\",\"code\":\"EIO\",\"message\":\"boom\"}\n",
    );

    let rendered = collect_agent_events_buffered(std::io::Cursor::new(input));

    assert!(matches!(
        rendered,
        Ok(ref rendered)
            if rendered.output == "\u{4f60}\u{597d}\n"
                && rendered.diagnostics
                    == vec![
                        "tool tsh running".to_owned(),
                        "tool tsh done 3 bytes".to_owned(),
                        "error EIO: boom".to_owned()
                    ]
                && rendered.exit_code == 1
                && !rendered.interrupted
    ));
}

#[test]
fn buffered_agent_renderer_reports_token_delta_and_total() {
    let input = concat!(
        "{\"type\":\"usage\",\"input_tokens\":10,\"output_tokens\":2}\n",
        "{\"type\":\"usage\",\"input_tokens\":4,\"output_tokens\":3}\n",
    );

    let rendered = collect_agent_events_buffered(std::io::Cursor::new(input));

    assert!(matches!(
        rendered,
        Ok(ref rendered)
            if rendered.diagnostics
                == vec![
                    "tokens in +10/10 out +2/2".to_owned(),
                    "tokens in +4/14 out +3/5".to_owned(),
                ]
    ));
}

#[test]
fn agent_renderer_waiting_diagnostic_is_readable() {
    assert_eq!(waiting_diagnostic(12), "waiting 12s for agent event");
}

#[test]
fn debug_tool_line_reports_current_names_and_changes() {
    assert_eq!(
        format_debug_tool_line(None, &["fs.read".to_owned(), "tsh".to_owned()]),
        "[debug tools] = fs.read tsh"
    );
    assert_eq!(
        format_debug_tool_line(
            Some(&["fs.read".to_owned(), "tsh".to_owned()]),
            &["fs.read".to_owned(), "fs.write".to_owned()]
        ),
        "[debug tools] +fs.write -tsh = fs.read fs.write"
    );
}

#[test]
fn debug_agent_send_request_marks_socket_frame() {
    let request = agent_send_request_json("run-1", "default", "/workspace", "hello", true);

    assert!(request.contains(r#""debug":true"#));
    assert!(request.ends_with('\n'));
}

#[test]
fn normal_agent_send_request_does_not_mark_socket_frame() {
    let request = agent_send_request_json("run-1", "default", "/workspace", "hello", false);

    assert!(!request.contains(r#""debug""#));
    assert!(request.ends_with('\n'));
}

#[test]
fn debug_timing_diagnostic_is_readable() {
    let value = serde_json::json!({
        "type": "debug",
        "stage": "first_model_frame",
        "elapsed_ms": 42
    });

    assert_eq!(
        debug_timing_diagnostic(&value),
        Some("[debug timing] +42ms first_model_frame".to_owned())
    );
}

#[test]
fn debug_tool_names_report_native_agent_tools_only() {
    let root = clean_test_dir("ctx-agent-debug-native-tools");
    assert!(ensure_v1_reference_tree(&root).is_ok());

    let tools = agent_native_tool_names(&root, "coder");

    assert_eq!(tools, Ok(vec!["tsh".to_owned()]));
}

#[test]
fn buffered_agent_renderer_rejects_too_much_output() {
    let chunk = "x".repeat(1024);
    let mut input = String::new();
    for _index in 0..(MAX_BUFFERED_AGENT_RENDERED_BYTES / chunk.len() + 2) {
        let _ignored = writeln!(input, "{{\"type\":\"delta\",\"text\":\"{chunk}\"}}");
    }

    let rendered = collect_agent_events_buffered(std::io::Cursor::new(input));

    assert!(matches!(rendered, Err(ref error) if error.message.contains("agent output exceeds")));
}

#[test]
fn streaming_agent_renderer_rejects_oversized_frame() {
    let input = format!("{}\n", "x".repeat(MAX_SOCKET_FRAME_BYTES));

    let rendered = render_agent_event_lines(std::io::Cursor::new(input), None);

    assert!(matches!(rendered, Err(ref error) if error.message.contains("cannot read socket response")));
}

#[test]
fn buffered_agent_renderer_rejects_oversized_frame_before_rendering() {
    let input = format!("{}\n", "x".repeat(MAX_SOCKET_FRAME_BYTES));

    let rendered = collect_agent_events_buffered(std::io::Cursor::new(input));

    assert!(matches!(rendered, Err(ref error) if error.message.contains("cannot read socket response")));
}

#[test]
fn buffered_agent_renderer_rejects_too_many_events() {
    let input = "{\"type\":\"ignored\"}\n".repeat(MAX_BUFFERED_AGENT_EVENTS + 1);

    let rendered = collect_agent_events_buffered(std::io::Cursor::new(input));

    assert!(matches!(rendered, Err(ref error) if error.message.contains("buffered events")));
}

#[test]
fn buffered_agent_renderer_rejects_too_many_diagnostics() {
    let input = "{\"type\":\"tool_call\",\"name\":\"tsh\"}\n"
        .repeat(MAX_BUFFERED_AGENT_DIAGNOSTICS + 1);

    let rendered = collect_agent_events_buffered(std::io::Cursor::new(input));

    assert!(matches!(rendered, Err(ref error) if error.message.contains("buffered diagnostics")));
}

#[test]
fn interruptible_agent_renderer_returns_on_interrupt_flag() {
    let pair = std::os::unix::net::UnixStream::pair();
    assert!(pair.is_ok());
    let Ok((reader, _writer)) = pair else {
        return;
    };
    assert!(reader
        .set_read_timeout(Some(std::time::Duration::from_millis(1)))
        .is_ok());
    let interrupted = std::sync::atomic::AtomicBool::new(true);

    let rendered =
        collect_agent_events_buffered_interruptible(std::io::BufReader::new(reader), &interrupted);

    assert!(matches!(
        rendered,
        Ok(ref rendered)
            if rendered.output.is_empty()
                && rendered.diagnostics.is_empty()
                && rendered.exit_code == 0
                && rendered.interrupted
    ));
}

#[test]
fn interruptible_raw_socket_copy_returns_on_interrupt_flag() {
    let pair = std::os::unix::net::UnixStream::pair();
    assert!(pair.is_ok());
    let Ok((reader, _writer)) = pair else {
        return;
    };
    assert!(reader
        .set_read_timeout(Some(std::time::Duration::from_millis(1)))
        .is_ok());
    let interrupted = std::sync::atomic::AtomicBool::new(true);

    let copied = copy_socket_response_interruptible(reader, &interrupted);

    assert!(matches!(copied, Ok(true)));
}

#[test]
fn interruptible_buffered_agent_request_sends_cancel_for_active_run() {
    let root = clean_test_dir("ctx-agent-repl-interrupt-cancel");
    assert!(fs::create_dir_all(&root).is_ok());
    let socket = root.join("agent.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket);
    assert!(listener.is_ok());
    let Ok(listener) = listener else {
        return;
    };

    let server = std::thread::spawn(move || {
        let first = listener.accept();
        assert!(first.is_ok());
        let Ok((mut first_stream, _addr)) = first else {
            return String::new();
        };
        let mut first_request = String::new();
        assert!(std::io::Read::read_to_string(&mut first_stream, &mut first_request).is_ok());

        let second = listener.accept();
        assert!(second.is_ok());
        let Ok((mut second_stream, _addr)) = second else {
            return first_request;
        };
        let mut second_request = String::new();
        assert!(std::io::Read::read_to_string(&mut second_stream, &mut second_request).is_ok());

        format!("{first_request}{second_request}")
    });

    let guard = AgentInterruptGuard::new();
    assert!(guard.is_ok());
    let Ok(guard) = guard else {
        return;
    };
    guard.interrupted_flag().store(true, std::sync::atomic::Ordering::SeqCst);

    let result = stream_agent_socket_request_buffered_interruptible(
        &socket,
        "{\"op\":\"send\",\"id\":\"run-1\"}\n",
        false,
        Some((&guard, "{\"op\":\"cancel\",\"id\":\"run-1\"}\n", "run-1")),
    );

    assert!(matches!(result, Ok(code) if code == std::process::ExitCode::SUCCESS));
    let requests = server.join();
    assert!(requests.is_ok());
    let Ok(requests) = requests else {
        return;
    };
    assert!(requests.contains("\"op\":\"send\""));
    assert!(requests.contains("\"op\":\"cancel\""));
    assert!(requests.contains("\"id\":\"run-1\""));
}

#[test]
fn agent_prompt_renders_runtime_system_prompt_from_control_files() {
    let root = clean_test_dir("ctx-agent-prompt-render");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let control = root.join("agent").join("coder.d");
    assert!(fs::write(control.join("system.md"), "Be precise.\n").is_ok());
    assert!(
        fs::write(
            control.join("prompt.template.md"),
            "agent={{agent}}\ntime={{current_time_unix}}\ninst={{agent_instructions}}\n{{runtime_contract}}\n",
        )
        .is_ok()
    );

    let prompt = build_agent_system_prompt(&root, "coder", "123");

    assert!(matches!(
        prompt,
        Ok(ref prompt)
            if prompt.contains("agent=coder")
                && prompt.contains("time=123")
                && prompt.contains("inst=Be precise.")
                && prompt.contains("Your only native callable tool is `tsh`")
                && !prompt.contains("{{agent}}")
    ));
}

#[test]
fn shell_quote_arg_escapes_single_quotes() {
    assert_eq!(shell_quote_arg("default"), "default");
    assert_eq!(shell_quote_arg("has space"), "'has space'");
    assert_eq!(shell_quote_arg("can't"), "'can'\\''t'");
}

#[test]
fn cli_names_accept_abi_valid_uppercase_names() {
    for name in ["NAME", "SESSION", "AGENT", "SOURCE", "TARGET", "PATH", "INPUT", "RUN"] {
        assert!(require_cli_name("agent name", name).is_ok(), "{name}");
        assert!(require_session_name(name).is_ok(), "{name}");
    }
}

#[test]
fn session_names_reject_control_characters() {
    for name in ["bad\rname", "bad\u{1b}name"] {
        assert!(require_session_name(name).is_err(), "{name:?}");
    }
}

fn contains_arg_pair(args: &[String], first: &str, second: &str) -> bool {
    args.windows(2)
        .any(|window| window.first().map(String::as_str) == Some(first)
            && window.get(1).map(String::as_str) == Some(second))
}

fn contains_arg_triplet(args: &[String], first: &str, second: &str, third: &str) -> bool {
    args.windows(3)
        .any(|window| window.first().map(String::as_str) == Some(first)
            && window.get(1).map(String::as_str) == Some(second)
            && window.get(2).map(String::as_str) == Some(third))
}

fn contains_ro_bind_stub(args: &[String], target: &str) -> bool {
    args.windows(3).any(|window| {
        window.first().map(String::as_str) == Some("--ro-bind")
            && window
                .get(1)
                .is_some_and(|source| source.ends_with("/.empty-shell-startup"))
            && window.get(2).map(String::as_str) == Some(target)
    })
}

fn create_agent_fixture(root: &Path, name: &str, parent: &str, status: &str, pid: &str) {
    let agent = fixture_path(root, &["agent", name]);
    write_text_file(&agent, "#!/bin/sh\nexit 0\n");
    let metadata = fs::metadata(&agent);
    assert!(metadata.is_ok());
    if let Ok(metadata) = metadata {
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        assert!(fs::set_permissions(&agent, permissions).is_ok());
    }
    let control = fixture_path(root, &["agent", &format!("{name}.d")]);
    write_text_file(&control.join("parent"), &newline_terminated(parent));
    write_text_file(&control.join("status"), &newline_terminated(status));
    write_text_file(&control.join("pid"), &newline_terminated(pid));
}

#[test]
fn parses_bootstrap_and_mount_commands() {
    let bootstrap = cmd!("bootstrap");
    assert!(matches!(bootstrap, Ok(Command::Bootstrap { source: None })));

    let update = cmd!("update");
    assert!(matches!(update, Ok(Command::Bootstrap { source: None })));

    let bootstrap_source = cmd!("bootstrap", "/tmp/cortexfs-source");
    assert!(matches!(
        bootstrap_source,
        Ok(Command::Bootstrap {
            source: Some(ref source)
        }) if source == Path::new("/tmp/cortexfs-source")
    ));

    let update_source = cmd!("update", "/tmp/cortexfs-source");
    assert!(matches!(
        update_source,
        Ok(Command::Bootstrap {
            source: Some(ref source)
        }) if source == Path::new("/tmp/cortexfs-source")
    ));

    let mount = cmd!(
        "mount",
        "--source",
        "/tmp/cortexfs-source",
        "/tmp/cortexfs-mount"
    );
    assert!(matches!(
        mount,
        Ok(Command::Mount {
            source: Some(ref source),
            mountpoint: Some(ref mountpoint)
        }) if source == Path::new("/tmp/cortexfs-source")
            && mountpoint == Path::new("/tmp/cortexfs-mount")
    ));
}

#[test]
fn parses_exec_command_with_arguments() {
    let command = cmd!("exec", "agent/coder", "fix tests");
    assert!(matches!(
        command,
        Ok(Command::Exec {
            ref path,
            ref args
        }) if path == "agent/coder" && args == &["fix tests".to_owned()]
    ));
}

#[test]
fn parses_tool_command_with_arguments() {
    let command = cmd!("tool", "fs.read", "README.md");
    assert!(matches!(
        command,
        Ok(Command::Tool {
            ref name,
            ref args
        }) if name == "fs.read" && args == &["README.md".to_owned()]
    ));
}

#[test]
fn parses_provider_oauth_commands() {
    let login = cmd!("provider", "oauth", "login", "api.openai.com", "--timeout", "30");
    assert!(matches!(
        login,
        Ok(Command::Provider(ProviderArgs::Login {
            ref provider,
            timeout
        })) if provider == "api.openai.com" && timeout == 30
    ));

    let status = cmd!("provider", "oauth", "status", "api.openai.com");
    assert!(matches!(
        status,
        Ok(Command::Provider(ProviderArgs::Status { ref provider }))
            if provider == "api.openai.com"
    ));

    let refresh = cmd!("provider", "oauth", "refresh", "api.openai.com");
    assert!(matches!(
        refresh,
        Ok(Command::Provider(ProviderArgs::Refresh { ref provider }))
            if provider == "api.openai.com"
    ));
}

#[test]
fn parses_provider_oauth_help_commands() {
    assert!(matches!(
        cmd!("provider", "--help"),
        Ok(Command::HelpTopic(ref topic)) if topic == "provider"
    ));
    assert!(matches!(
        cmd!("provider", "oauth", "--help"),
        Ok(Command::HelpTopic(ref topic)) if topic == "provider oauth"
    ));
    assert!(matches!(
        cmd!("provider", "oauth", "login", "--help"),
        Ok(Command::HelpTopic(ref topic)) if topic == "provider oauth login"
    ));
}

#[test]
fn parses_provider_preset_commands() {
    assert!(matches!(
        cmd!("provider", "preset", "list"),
        Ok(Command::Provider(ProviderArgs::PresetList))
    ));
    assert!(matches!(
        cmd!("provider", "preset", "show", "google"),
        Ok(Command::Provider(ProviderArgs::PresetShow { ref preset }))
            if preset == "google"
    ));
    assert!(matches!(
        cmd!("provider", "preset", "install", "anthropic"),
        Ok(Command::Provider(ProviderArgs::PresetInstall { ref preset }))
            if preset == "anthropic"
    ));
}

#[test]
fn parses_provider_secret_commands() {
    assert!(matches!(
        cmd!("provider", "secret", "set", "local"),
        Ok(Command::Provider(ProviderArgs::SecretSet {
            ref provider,
            ref slot
        })) if provider == "local" && slot == "default"
    ));
    assert!(matches!(
        cmd!("provider", "secret", "status", "openai", "--slot", "office"),
        Ok(Command::Provider(ProviderArgs::SecretStatus {
            ref provider,
            ref slot
        })) if provider == "openai" && slot == "office"
    ));
}

#[test]
fn tool_command_runs_core_tool_cli_at_selected_root() {
    let root = clean_test_dir("ctx-tool-command-core");
    assert!(ensure_v1_reference_tree(&root).is_ok());

    let mut output = Vec::new();
    let result = run_visible_tool_with_writer(
        &root,
        "tsh.config",
        &[r#"{"max_loaded_tools":9,"cache_capacity":4,"window_percent":2}"#.to_owned()],
        &mut output,
    );

    assert_eq!(result, Ok(std::process::ExitCode::SUCCESS));
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains("tool/tsh.d/config"));
    let config = fs::read_to_string(root.join("tool/tsh.d/config")).unwrap_or_default();
    assert!(config.contains("max_loaded_tools=9\n"));
    assert!(config.contains("cache_capacity=4\n"));
    assert!(config.contains("window_percent=2\n"));
}

#[test]
fn tool_command_requires_core_tool_to_be_visible() {
    let root = clean_test_dir("ctx-tool-command-core-hidden");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    assert!(fs::remove_file(root.join("tool").join("tsh.config")).is_ok());

    let result = run_visible_tool_with_writer(
        &root,
        "tsh.config",
        &[r#"{"max_loaded_tools":9}"#.to_owned()],
        &mut Vec::new(),
    );

    assert!(matches!(
        result,
        Err(ref error)
            if error.code == 69 && error.message.contains("tool not found in CTX_PATH: tsh.config")
    ));
}

#[test]
fn tool_command_refuses_authority_bearing_core_tool_cli() {
    let root = clean_test_dir("ctx-tool-command-core-authority");
    let tool = root.join("tool").join("fs.write");
    write_text_file(&tool, "#!/bin/sh\nexit 7\n");
    assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());
    let blocked_path = root.join("blocked-output");

    let result = run_visible_tool_with_writer(
        &root,
        "fs.write",
        &[blocked_path.display().to_string(), "blocked".to_owned()],
        &mut Vec::new(),
    );

    assert!(matches!(
        result,
        Err(ref error)
            if error.code == 69
                && error.message.contains("direct CTX_PATH execution bypasses CortexFS tool authorization")
    ));
    assert!(!blocked_path.exists());
}

#[test]
fn tool_command_refuses_direct_ctx_path_execution() {
    let root = clean_test_dir("ctx-tool-command-visible");
    let tool = root.join("tool").join("project.echo");
    write_text_file(&tool, "#!/bin/sh\nexit 7\n");
    assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());

    let result = run_visible_tool(&root, "project.echo", &["hello".to_owned()]);
    assert!(matches!(
        result,
        Err(ref error)
            if error.code == 69
                && error.message.contains("direct CTX_PATH execution bypasses CortexFS tool authorization")
    ));
}

#[test]
fn abi_path_resolution_rejects_escape() {
    let root = Path::new("/ctx");
    assert!(resolve_abi_path(root, "agent/coder.d/cwd").is_ok());
    assert!(classify_input_path(root, "agent/coder.d/cwd").is_ok());
    assert!(resolve_abi_path(root, "../etc/passwd").is_err());
    assert!(classify_input_path(root, "../etc/passwd").is_err());
    assert!(classify_input_path(root, "agent//coder").is_err());
    assert!(resolve_abi_path(root, "agent/coder\u{1b}").is_err());
    assert!(classify_input_path(root, "agent/coder\u{1b}").is_err());
    assert!(resolve_abi_path(root, "/etc/passwd").is_err());
    assert!(resolve_abi_path(root, "/ctx/../etc/passwd").is_err());
    assert!(classify_input_path(root, "/ctx/agent/coder\u{1b}").is_err());
    assert_eq!(
        resolve_abi_path(root, "/ctx/agent/coder.d/cwd").map(|path| path.display().to_string()),
        Ok("/ctx/agent/coder.d/cwd".to_owned())
    );
}

#[test]
fn ls_lists_abi_paths_and_keeps_object_filtering() {
    let root = clean_test_dir("ctx-ls-paths");
    assert!(ensure_v1_reference_tree(&root).is_ok());

    let home = list_names(&root, &LsTarget::Path("home".to_owned()));
    assert_eq!(home, Ok(vec!["1000".to_owned()]));

    let root_alias = list_names(&root, &LsTarget::Path("/".to_owned()));
    assert!(matches!(root_alias, Ok(ref names) if names.contains(&"home".to_owned())));

    let absolute_home = root.join("home");
    let absolute_home = absolute_home.display().to_string();
    let home_absolute = list_names(&root, &LsTarget::Path(absolute_home));
    assert_eq!(home_absolute, Ok(vec!["1000".to_owned()]));

    let absolute_escape = root.join("../outside").display().to_string();
    assert!(list_names(&root, &LsTarget::Path(absolute_escape)).is_err());

    let tool = list_names(&root, &LsTarget::Path("tool".to_owned()));
    assert!(matches!(
        tool,
        Ok(ref names)
            if names.contains(&"tsh".to_owned()) && !names.contains(&"tsh.d".to_owned())
    ));
}

#[test]
fn ls_rejects_symlink_directories_without_listing_targets() {
    let root = clean_test_dir("ctx-ls-symlink-directory");
    let outside = clean_test_dir("ctx-ls-symlink-directory-outside");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    assert!(fs::remove_dir_all(root.join("home")).is_ok());
    assert!(fs::create_dir_all(outside.join("1000")).is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("home")).is_ok());

    assert!(list_names(&root, &LsTarget::Path("home".to_owned())).is_err());
}

#[test]
fn detects_durable_session_instance_paths() {
    assert_path_matches(
        &[
            "home/1000/agent/coder/session/default",
            "shared/im-qq-dev/agent/bot/session/group-456",
            "home/1000/model/openai/gpt-4o.d/session/default",
            "shared/project-a/model/openai/gpt-4o.d/session/default",
        ],
        is_durable_session_instance_path,
        true,
    );
    assert_path_matches(
        &[
            "home/1000/agent/coder/session",
            "home/1000/agent/coder/session/default/messages.jsonl",
            "shared/project-a/model/openai/gpt-4o/session/default",
        ],
        is_durable_session_instance_path,
        false,
    );
}

#[test]
fn detects_session_control_paths() {
    for (path, expected) in [
        (
            "home/1000/agent/coder/session/default/state",
            Some(SessionControlKind::State),
        ),
        (
            "shared/im-qq-dev/agent/bot/session/group-456/cwd",
            Some(SessionControlKind::Cwd),
        ),
        (
            "home/1000/model/openai/gpt-4o.d/session/default/meta.json",
            Some(SessionControlKind::MetaJson),
        ),
        ("home/1000/agent/coder/session/default/messages.jsonl", None),
    ] {
        assert_path_kind!(path, session_control_path_kind, expected);
    }
}

#[test]
fn detects_private_and_shared_context_pack_paths() {
    assert_path_matches(
        &[
            "home/1000/agent/coder/session/default/context/pack.json",
            "shared/im-qq-dev/agent/bot/session/group-456/context/pack.json",
        ],
        is_context_pack_path,
        true,
    );
    assert_path_matches(
        &[
            "home/1000/agent/coder/session/default/context/pack.md",
            "home/1000/agent/bad/name/session/default/context/pack.json",
        ],
        is_context_pack_path,
        false,
    );
}

#[test]
fn detects_private_and_shared_event_stream_paths() {
    assert_path_matches(
        &[
            "home/1000/agent/coder/session/default/events.jsonl",
            "shared/im-qq-dev/agent/bot/session/group-456/events.jsonl",
            "home/1000/model/openai/gpt-4o.d/session/default/events.jsonl",
            "shared/project-a/model/openai/gpt-4o.d/session/default/events.jsonl",
        ],
        is_session_events_path,
        true,
    );
    assert_path_matches(
        &[
            "home/1000/agent/coder/session/default/messages.jsonl",
            "shared/im-qq-dev/agent/bad/name/session/group-456/events.jsonl",
        ],
        is_session_events_path,
        false,
    );
}

#[test]
fn detects_private_and_shared_message_stream_paths() {
    assert_path_matches(
        &[
            "home/1000/agent/coder/session/default/messages.jsonl",
            "shared/im-qq-dev/agent/bot/session/group-456/messages.jsonl",
            "home/1000/model/openai/gpt-4o.d/session/default/messages.jsonl",
        ],
        is_session_messages_path,
        true,
    );
    assert!(!is_session_messages_path(
        "home/1000/agent/coder/session/default/events.jsonl"
    ));
}

#[test]
fn detects_context_jsonl_paths() {
    for (path, expected) in [
        (
            "home/1000/agent/coder/session/default/context/facts.jsonl",
            Some(ContextJsonlKind::Facts),
        ),
        (
            "shared/im-qq-dev/agent/bot/session/group-456/context/decisions.jsonl",
            Some(ContextJsonlKind::Decisions),
        ),
        (
            "home/1000/model/openai/gpt-4o.d/session/default/context/swap/index.jsonl",
            Some(ContextJsonlKind::SwapIndex),
        ),
        (
            "shared/project-a/model/openai/gpt-4o.d/session/default/context/dedup/index.jsonl",
            Some(ContextJsonlKind::DedupIndex),
        ),
        ("home/1000/agent/coder/session/default/context/pack.json", None),
    ] {
        assert_path_kind!(path, context_jsonl_path_kind, expected);
    }
}

#[test]
fn detects_private_and_shared_session_index_paths() {
    for (path, expected) in [
        (
            "home/1000/agent/coder/session/index/list",
            Some(SessionIndexKind::List),
        ),
        (
            "home/1000/agent/coder/session/index/current",
            Some(SessionIndexKind::Current),
        ),
        (
            "shared/im-qq-dev/agent/bot/session/index/by-cwd/hash-1",
            Some(SessionIndexKind::ByCwd),
        ),
        (
            "home/1000/agent/coder/session/index/by-hash/hash-1",
            Some(SessionIndexKind::ByHash),
        ),
        (
            "home/1000/agent/coder/session/index/by-uuid/uuid-1",
            Some(SessionIndexKind::ByUuid),
        ),
        ("home/1000/agent/coder/session/index/by-hash/bad:key", None),
        ("home/1000/agent/coder/session/default", None),
        ("home/1000/agent/bad/name/session/index/list", None),
    ] {
        assert_path_kind!(path, session_index_path_kind, expected);
    }
}

#[test]
fn detects_executable_object_paths() {
    for (path, expected) in [
        (
            "model/openai/gpt-4o",
            Some((ObjectClass::Model, "openai/gpt-4o".to_owned())),
        ),
        ("agent/coder", Some((ObjectClass::Agent, "coder".to_owned()))),
        ("tool/fs.read", Some((ObjectClass::Tool, "fs.read".to_owned()))),
        ("tool/fs.read.d/schema", None),
        ("home/1000", None),
    ] {
        assert_path_kind!(path, executable_object_path, expected);
    }
}

#[test]
fn detects_model_capability_paths() {
    assert_path_matches(
        &["model/openai/gpt-4o.d/cap", "model/google/gemini-2.5-pro.d/cap"],
        is_model_capability_path,
        true,
    );
    assert_path_matches(
        &["tool/fs.read.d/cap", "model/openai/gpt-4o/cap", "model/openai/gpt-4o.d/native"],
        is_model_capability_path,
        false,
    );
}

#[test]
fn detects_model_driver_paths() {
    assert_path_matches(
        &["model/openai/gpt-4o.d/driver", "model/anthropic/claude-sonnet-4.d/driver"],
        is_model_driver_path,
        true,
    );
    assert_path_matches(
        &["model/openai/gpt-4o/driver", "model/openai/gpt-4o.d/cap"],
        is_model_driver_path,
        false,
    );
}

#[test]
fn detects_tool_schema_paths() {
    assert_path_matches(
        &["tool/fs.read.d/schema", "tool/mcp.github.search_issues.d/schema"],
        is_tool_schema_path,
        true,
    );
    assert_path_matches(
        &["tool/fs.read/schema", "model/openai/gpt-4o.d/schema", "tool/bad/name.d/schema"],
        is_tool_schema_path,
        false,
    );
}

#[test]
fn detects_shared_tool_schema_paths() {
    assert_path_matches(
        &[
            "shared/project-a/tool/project.test.d/schema",
            "shared/project-a/tool/mcp.github.search_issues.d/schema",
        ],
        is_shared_tool_schema_path,
        true,
    );
    assert_path_matches(
        &[
            "shared/project-a/tool/project.test.d/policy",
            "tool/project.test.d/schema",
            "shared/project-a/tool/bad/name.d/schema",
        ],
        is_shared_tool_schema_path,
        false,
    );
}

#[test]
fn detects_shared_queue_root_paths() {
    assert_path_matches(
        &["shared/project-a/queue", "shared/im-qq-dev/queue"],
        is_shared_queue_root_path,
        true,
    );
    assert_path_matches(
        &["shared/project-a/queue/pending", "shared/project-a/result", "shared/bad/name/queue"],
        is_shared_queue_root_path,
        false,
    );
}

#[test]
fn detects_agent_control_paths_with_fixed_value_syntax() {
    for (path, expected) in [
        ("agent/coder.d/uid", Some(AgentControlKind::Uid)),
        ("agent/coder.d/life", Some(AgentControlKind::Life)),
        ("agent/rev-1.d/parent", Some(AgentControlKind::Parent)),
        ("agent/coder.d/label", None),
        ("model/openai/gpt-4o.d/session", None),
        ("agent/bad/name.d/uid", None),
    ] {
        assert_path_kind!(path, agent_control_path_kind, expected);
    }
}

#[test]
fn ctx_env_quotes_path_export_root_bin() {
    let exports = env_exports(
        Path::new("/tmp/ctx;echo CORTEXFS_CTX_ENV_EVAL_PWNED >/tmp/pwn #"),
        None,
        None,
    );

    assert_eq!(
        exports[3],
        "export PATH='/tmp/ctx;echo CORTEXFS_CTX_ENV_EVAL_PWNED >/tmp/pwn #/bin':$PATH"
    );
}

#[test]
fn ctx_env_escapes_terminal_controls_in_exports() {
    let exports = env_exports(
        Path::new("/tmp/ctx\u{1b}]52;c;payload\u{7}"),
        Some("/home/user\u{1b}[31m"),
        None,
    );

    assert!(exports.iter().all(|line| !line.as_bytes().contains(&0x1b)));
    assert!(exports.iter().all(|line| !line.as_bytes().contains(&0x07)));
    assert!(exports[0].contains("\\u{1b}]52;c;payload\\u{7}"));
    assert!(exports[1].contains("\\u{1b}[31m"));
}

#[test]
fn ctx_env_preserves_path_expansion_for_safe_root() {
    let exports = env_exports(Path::new("/ctx"), None, None);

    assert_eq!(exports[3], "export PATH=/ctx/bin:$PATH");
}
