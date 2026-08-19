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
    assert!(matches!(
        cmd!("history", "coder", "focus"),
        Err(ref error) if error.code == 2 && error.message.contains("unexpected argument")
    ));

    let output = cmd!("agent", "output", "coder", "--session", "default");
    assert!(matches!(
        output,
        Ok(Command::Agent(AgentArgs::Output {
            ref name,
            session: Some(ref session)
        })) if name == "coder" && session == "default"
    ));

    let wait = cmd!("agent", "wait", "coder", "work-123", "--session", "default");
    assert!(matches!(
        wait,
        Ok(Command::Agent(AgentArgs::Wait {
            ref name,
            session: Some(ref session),
            ref child,
        })) if name == "coder" && session == "default" && child == "work-123"
    ));

    let prompt = cmd!("agent", "prompt", "coder");
    assert!(matches!(
        prompt,
        Ok(Command::Agent(AgentArgs::Prompt { ref name })) if name == "coder"
    ));

    let resume = cmd!("resume", "coder", "default");
    assert!(matches!(
        resume,
        Err(ref error) if error.code == 2 && error.message.contains("unexpected argument")
    ));

    let resume_flagged = cmd!("resume", "coder", "-s", "focus");
    assert!(matches!(
        resume_flagged,
        Ok(Command::Resume {
            agent: Some(ref agent),
            session: Some(ref session)
        }) if agent == "coder" && session == "focus"
    ));

    let send = cmd!("send", "coder", "hello", "world");
    assert!(matches!(
        send,
        Ok(Command::Send {
            ref agent,
            session: None,
            ref input
        }) if agent == "coder" && input == "hello world"
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

    let agent_chat = cmd!("agent", "chat", "--help");
    assert!(matches!(
        agent_chat,
        Ok(Command::HelpTopic(ref topic)) if topic == "agent chat"
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
    let send = cmd!(
        "agent",
        "send",
        "coder",
        "--session",
        "test",
        "hello",
        "world"
    );
    assert!(matches!(
        send,
        Ok(Command::Agent(AgentArgs::Send {
            ref name,
            session: Some(ref session),
            ref input,
            raw: false,
            ..
        })) if name == "coder" && session == "test" && input == "hello world"
    ));

    let repl = cmd!("agent", "repl", "coder", "--raw");
    assert!(matches!(
        repl,
        Err(ref error) if error.code == 2 && error.message == "unknown agent command: repl"
    ));

    let chat = cmd!("agent", "chat", "coder", "--session", "focus");
    assert!(matches!(
        chat,
        Ok(Command::Agent(AgentArgs::Chat {
            ref name,
            session: Some(ref session),
            raw: false,
            ..
        })) if name == "coder" && session == "focus"
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
fn agent_send_rejects_invalid_approval_during_parse() {
    let command = cmd!("agent", "send", "coder", "--approve", "bad/name", "hello");

    assert!(matches!(
        command,
        Err(ref error) if error.code == 2 && error.message.contains("approved tool name")
    ));
}

#[test]
fn agent_chat_rejects_invalid_approval_during_parse() {
    let command = cmd!("agent", "chat", "coder", "--approve", "bad/name");

    assert!(matches!(
        command,
        Err(ref error) if error.code == 2 && error.message.contains("approved tool name")
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
fn agent_trajectory_error_lists_actionable_projection_issues() {
    let root = clean_test_dir("ctx-agent-trajectory-invalid");
    create_agent_fixture(&root, "coder", "agent:base", "idle", "");
    let session = ctx_home(&root)
        .unwrap_or_default()
        .join("agent/coder/session/default");
    create_complete_session_layout(&session);
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"hello\"}\n",
    );
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(&session.join("meta.json"), "{\"client\":\"\"}\n");

    let result = agent_trajectory(&root, "coder", Some("default"));

    assert!(matches!(
        result,
        Err(ref error)
            if error.code == 69
                && error.message
                    == "invalid trajectory projection (1 issues)\n- missing agent name"
    ));
}

#[test]
fn trajectory_cli_issue_text_escapes_and_bounds_session_source() {
    let source = format!("bad\n\u{1b}[31m{}", "x".repeat(2_000));
    let issue = TrajectoryIssue::InvalidStepSource { index: 0, source };

    let rendered = format_trajectory_issue(&issue);

    assert!(!rendered.contains('\n'));
    assert!(!rendered.contains('\u{1b}'));
    assert!(rendered.contains("\\n"));
    assert!(rendered.contains("\\u{1b}"));
    assert!(rendered.chars().count() <= 256, "{}", rendered.len());
}

#[test]
fn agent_send_prompt_root_words_do_not_override_selected_root() {
    let root = Path::new("/tmp/cortexfs-selected-root");
    let attacker_root = Path::new("/tmp/cortexfs-attacker-root");

    let command = parse_cli_args(&[
        "--root",
        root.to_str().unwrap_or_default(),
        "agent",
        "send",
        "coder",
        "token",
        "--root",
        attacker_root.to_str().unwrap_or_default(),
        "secret",
    ]);

    assert!(matches!(
        command,
        Ok(Cli {
            root: ref parsed_root,
            command: Command::Agent(AgentArgs::Send {
                ref name,
                session: None,
                raw: false,
                ref input,
                ..
            }),
        }) if parsed_root == root
            && name == "coder"
            && input == &format!("token --root {} secret", attacker_root.display())
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
            ("CTX_ROOT".to_owned(), Some(root.display().to_string())),
            ("PATH".to_owned(), Some("/usr/bin:/bin".to_owned())),
        ]
    );
}
