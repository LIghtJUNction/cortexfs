#[test]
fn parses_spec_which_command() {
    let command = parse_command(vec![
        "which".to_owned(),
        "tool".to_owned(),
        "fs.read".to_owned(),
    ]);
    assert!(matches!(
        command,
        Ok(Command::Which(ObjectClass::Tool, ref name)) if name == "fs.read"
    ));
}

#[test]
fn parses_file_set_command() {
    let command = parse_command(vec![
        "file".to_owned(),
        "set".to_owned(),
        "agent/coder.d/cwd".to_owned(),
        "/work".to_owned(),
    ]);
    assert!(matches!(
        command,
        Ok(Command::File(ref args))
            if args.command == FileCommand::Set
                && args.path == "agent/coder.d/cwd"
                && args.value.as_deref() == Some("/work")
    ));
}

#[test]
fn parses_file_classify_command() {
    let explicit = parse_command(vec![
        "file".to_owned(),
        "classify".to_owned(),
        "tool/fs.read".to_owned(),
    ]);
    assert!(matches!(
        explicit,
        Ok(Command::File(ref args))
            if args.command == FileCommand::Classify
                && args.path == "tool/fs.read"
                && args.value.is_none()
    ));

    let shorthand = parse_command(vec!["file".to_owned(), "tool/fs.read".to_owned()]);
    assert!(matches!(
        shorthand,
        Ok(Command::File(ref args))
            if args.command == FileCommand::Classify
                && args.path == "tool/fs.read"
                && args.value.is_none()
    ));
}

#[test]
fn parses_ls_path_command() {
    let root = parse_command(vec!["ls".to_owned()]);
    assert!(matches!(root, Ok(Command::Ls(LsTarget::Root))));

    let home = parse_command(vec!["ls".to_owned(), "home".to_owned()]);
    assert!(matches!(
        home,
        Ok(Command::Ls(LsTarget::Path(ref path))) if path == "home"
    ));

    let tool = parse_command(vec!["ls".to_owned(), "tool".to_owned()]);
    assert!(matches!(
        tool,
        Ok(Command::Ls(LsTarget::Path(ref path))) if path == "tool"
    ));
}

#[test]
fn parses_session_file_commands() {
    let history = parse_command(vec!["history".to_owned(), "coder".to_owned()]);
    assert!(matches!(
        history,
        Ok(Command::History {
            ref agent,
            session: None
        }) if agent == "coder"
    ));

    let latest = parse_command(vec![
        "latest".to_owned(),
        "coder".to_owned(),
        "default".to_owned(),
    ]);
    assert!(matches!(
        latest,
        Ok(Command::Latest {
            ref agent,
            session: Some(ref session)
        }) if agent == "coder" && session == "default"
    ));

    let resume = parse_command(vec![
        "resume".to_owned(),
        "coder".to_owned(),
        "default".to_owned(),
    ]);
    assert!(matches!(
        resume,
        Ok(Command::Resume {
            ref agent,
            session: Some(ref session)
        }) if agent == "coder" && session == "default"
    ));

    let send = parse_command(vec![
        "send".to_owned(),
        "coder".to_owned(),
        "default".to_owned(),
        "hello".to_owned(),
    ]);
    assert!(matches!(
        send,
        Ok(Command::Send {
            ref agent,
            ref session,
            ref input
        }) if agent == "coder" && session == "default" && input == "hello"
    ));

    let ping = parse_command(vec!["ping".to_owned(), "agent/coder".to_owned()]);
    assert!(matches!(
        ping,
        Ok(Command::Ping { ref path }) if path == "agent/coder"
    ));

    let cancel = parse_command(vec![
        "cancel".to_owned(),
        "agent/coder".to_owned(),
        "run-1".to_owned(),
    ]);
    assert!(matches!(
        cancel,
        Ok(Command::Cancel { ref path, ref run }) if path == "agent/coder" && run == "run-1"
    ));
}

#[test]
fn parses_bootstrap_and_mount_commands() {
    let bootstrap = parse_command(vec!["bootstrap".to_owned()]);
    assert!(matches!(bootstrap, Ok(Command::Bootstrap { source: None })));

    let bootstrap_source = parse_command(vec![
        "bootstrap".to_owned(),
        "/tmp/cortexfs-source".to_owned(),
    ]);
    assert!(matches!(
        bootstrap_source,
        Ok(Command::Bootstrap {
            source: Some(ref source)
        }) if source == Path::new("/tmp/cortexfs-source")
    ));

    let mount = parse_command(vec![
        "mount".to_owned(),
        "--source".to_owned(),
        "/tmp/cortexfs-source".to_owned(),
        "/tmp/cortexfs-mount".to_owned(),
    ]);
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
    let command = parse_command(vec![
        "exec".to_owned(),
        "agent/coder".to_owned(),
        "fix tests".to_owned(),
    ]);
    assert!(matches!(
        command,
        Ok(Command::Exec {
            ref path,
            ref args
        }) if path == "agent/coder" && args == &["fix tests".to_owned()]
    ));
}

#[test]
fn abi_path_resolution_rejects_escape() {
    let root = Path::new("/ctx");
    assert!(resolve_abi_path(root, "agent/coder.d/cwd").is_ok());
    assert!(resolve_abi_path(root, "../etc/passwd").is_err());
    assert!(resolve_abi_path(root, "/etc/passwd").is_err());
    assert_eq!(
        resolve_abi_path(root, "/ctx/agent/coder.d/cwd").map(|path| path.display().to_string()),
        Ok("/ctx/agent/coder.d/cwd".to_owned())
    );
}

#[test]
fn ls_lists_abi_paths_and_keeps_object_filtering() {
    let root = unique_test_dir("ctx-ls-paths");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(ensure_v1_reference_tree(&root).is_ok());

    let home = list_names(&root, &LsTarget::Path("home".to_owned()));
    assert_eq!(home, Ok(vec!["1000".to_owned()]));

    let root_alias = list_names(&root, &LsTarget::Path("/".to_owned()));
    assert!(matches!(root_alias, Ok(ref names) if names.contains(&"home".to_owned())));

    let absolute_home = root.join("home");
    let absolute_home = absolute_home.display().to_string();
    let home_absolute = list_names(&root, &LsTarget::Path(absolute_home));
    assert_eq!(home_absolute, Ok(vec!["1000".to_owned()]));

    let tool = list_names(&root, &LsTarget::Path("tool".to_owned()));
    assert!(matches!(
        tool,
        Ok(ref names)
            if names.contains(&"fs.read".to_owned())
                && !names.contains(&"fs.read.d".to_owned())
    ));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn detects_durable_session_instance_paths() {
    assert!(is_durable_session_instance_path(
        "home/1000/agent/coder/session/default"
    ));
    assert!(is_durable_session_instance_path(
        "shared/im-qq-dev/agent/bot/session/group-456"
    ));
    assert!(is_durable_session_instance_path(
        "home/1000/model/openai/gpt-4o.d/session/default"
    ));
    assert!(is_durable_session_instance_path(
        "shared/project-a/model/openai/gpt-4o.d/session/default"
    ));
    assert!(!is_durable_session_instance_path(
        "home/1000/agent/coder/session"
    ));
    assert!(!is_durable_session_instance_path(
        "home/1000/agent/coder/session/default/messages.jsonl"
    ));
    assert!(!is_durable_session_instance_path(
        "shared/project-a/model/openai/gpt-4o/session/default"
    ));
}

#[test]
fn detects_session_control_paths() {
    assert_eq!(
        session_control_path_kind("home/1000/agent/coder/session/default/state"),
        Some(SessionControlKind::State)
    );
    assert_eq!(
        session_control_path_kind("shared/im-qq-dev/agent/bot/session/group-456/cwd"),
        Some(SessionControlKind::Cwd)
    );
    assert_eq!(
        session_control_path_kind("home/1000/model/openai/gpt-4o.d/session/default/meta.json"),
        Some(SessionControlKind::MetaJson)
    );
    assert_eq!(
        session_control_path_kind("home/1000/agent/coder/session/default/messages.jsonl"),
        None
    );
}

#[test]
fn detects_private_and_shared_context_pack_paths() {
    assert!(is_context_pack_path(
        "home/1000/agent/coder/session/default/context/pack.json"
    ));
    assert!(is_context_pack_path(
        "shared/im-qq-dev/agent/bot/session/group-456/context/pack.json"
    ));
    assert!(!is_context_pack_path(
        "home/1000/agent/coder/session/default/context/pack.md"
    ));
    assert!(!is_context_pack_path(
        "home/1000/agent/bad/name/session/default/context/pack.json"
    ));
}

#[test]
fn detects_private_and_shared_event_stream_paths() {
    assert!(is_session_events_path(
        "home/1000/agent/coder/session/default/events.jsonl"
    ));
    assert!(is_session_events_path(
        "shared/im-qq-dev/agent/bot/session/group-456/events.jsonl"
    ));
    assert!(is_session_events_path(
        "home/1000/model/openai/gpt-4o.d/session/default/events.jsonl"
    ));
    assert!(is_session_events_path(
        "shared/project-a/model/openai/gpt-4o.d/session/default/events.jsonl"
    ));
    assert!(!is_session_events_path(
        "home/1000/agent/coder/session/default/messages.jsonl"
    ));
    assert!(!is_session_events_path(
        "shared/im-qq-dev/agent/bad/name/session/group-456/events.jsonl"
    ));
}

#[test]
fn detects_private_and_shared_message_stream_paths() {
    assert!(is_session_messages_path(
        "home/1000/agent/coder/session/default/messages.jsonl"
    ));
    assert!(is_session_messages_path(
        "shared/im-qq-dev/agent/bot/session/group-456/messages.jsonl"
    ));
    assert!(is_session_messages_path(
        "home/1000/model/openai/gpt-4o.d/session/default/messages.jsonl"
    ));
    assert!(!is_session_messages_path(
        "home/1000/agent/coder/session/default/events.jsonl"
    ));
}

#[test]
fn detects_context_jsonl_paths() {
    assert_eq!(
        context_jsonl_path_kind("home/1000/agent/coder/session/default/context/facts.jsonl"),
        Some(ContextJsonlKind::Facts)
    );
    assert_eq!(
        context_jsonl_path_kind(
            "shared/im-qq-dev/agent/bot/session/group-456/context/decisions.jsonl"
        ),
        Some(ContextJsonlKind::Decisions)
    );
    assert_eq!(
        context_jsonl_path_kind(
            "home/1000/model/openai/gpt-4o.d/session/default/context/swap/index.jsonl"
        ),
        Some(ContextJsonlKind::SwapIndex)
    );
    assert_eq!(
        context_jsonl_path_kind(
            "shared/project-a/model/openai/gpt-4o.d/session/default/context/dedup/index.jsonl"
        ),
        Some(ContextJsonlKind::DedupIndex)
    );
    assert_eq!(
        context_jsonl_path_kind("home/1000/agent/coder/session/default/context/pack.json"),
        None
    );
}

#[test]
fn detects_private_and_shared_session_index_paths() {
    assert_eq!(
        session_index_path_kind("home/1000/agent/coder/session/index/list"),
        Some(SessionIndexKind::List)
    );
    assert_eq!(
        session_index_path_kind("home/1000/agent/coder/session/index/current"),
        Some(SessionIndexKind::Current)
    );
    assert_eq!(
        session_index_path_kind("shared/im-qq-dev/agent/bot/session/index/by-cwd/hash-1"),
        Some(SessionIndexKind::ByCwd)
    );
    assert_eq!(
        session_index_path_kind("home/1000/agent/coder/session/default"),
        None
    );
    assert_eq!(
        session_index_path_kind("home/1000/agent/bad/name/session/index/list"),
        None
    );
}

#[test]
fn detects_executable_object_paths() {
    assert_eq!(
        executable_object_path("model/openai/gpt-4o"),
        Some((ObjectClass::Model, "openai/gpt-4o".to_owned()))
    );
    assert_eq!(
        executable_object_path("agent/coder"),
        Some((ObjectClass::Agent, "coder".to_owned()))
    );
    assert_eq!(
        executable_object_path("tool/fs.read"),
        Some((ObjectClass::Tool, "fs.read".to_owned()))
    );
    assert_eq!(executable_object_path("tool/fs.read.d/schema"), None);
    assert_eq!(executable_object_path("home/1000"), None);
}

#[test]
fn detects_model_capability_paths() {
    assert!(is_model_capability_path("model/openai/gpt-4o.d/cap"));
    assert!(is_model_capability_path("model/google/gemini-2.5-pro.d/cap"));
    assert!(!is_model_capability_path("tool/fs.read.d/cap"));
    assert!(!is_model_capability_path("model/openai/gpt-4o/cap"));
    assert!(!is_model_capability_path("model/openai/gpt-4o.d/native"));
}

#[test]
fn detects_model_driver_paths() {
    assert!(is_model_driver_path("model/openai/gpt-4o.d/driver"));
    assert!(is_model_driver_path("model/anthropic/claude-sonnet-4.d/driver"));
    assert!(!is_model_driver_path("model/openai/gpt-4o/driver"));
    assert!(!is_model_driver_path("model/openai/gpt-4o.d/cap"));
}

#[test]
fn detects_tool_schema_paths() {
    assert!(is_tool_schema_path("tool/fs.read.d/schema"));
    assert!(is_tool_schema_path(
        "tool/mcp.github.search_issues.d/schema"
    ));
    assert!(!is_tool_schema_path("tool/fs.read/schema"));
    assert!(!is_tool_schema_path("model/openai/gpt-4o.d/schema"));
    assert!(!is_tool_schema_path("tool/bad/name.d/schema"));
}

#[test]
fn detects_shared_tool_schema_paths() {
    assert!(is_shared_tool_schema_path(
        "shared/project-a/tool/project.test.d/schema"
    ));
    assert!(is_shared_tool_schema_path(
        "shared/project-a/tool/mcp.github.search_issues.d/schema"
    ));
    assert!(!is_shared_tool_schema_path(
        "shared/project-a/tool/project.test.d/policy"
    ));
    assert!(!is_shared_tool_schema_path("tool/project.test.d/schema"));
    assert!(!is_shared_tool_schema_path(
        "shared/project-a/tool/bad/name.d/schema"
    ));
}

#[test]
fn detects_shared_queue_root_paths() {
    assert!(is_shared_queue_root_path("shared/project-a/queue"));
    assert!(is_shared_queue_root_path("shared/im-qq-dev/queue"));
    assert!(!is_shared_queue_root_path("shared/project-a/queue/pending"));
    assert!(!is_shared_queue_root_path("shared/project-a/result"));
    assert!(!is_shared_queue_root_path("shared/bad/name/queue"));
}

#[test]
fn detects_agent_control_paths_with_fixed_value_syntax() {
    assert_eq!(
        agent_control_path_kind("agent/coder.d/uid"),
        Some(AgentControlKind::Uid)
    );
    assert_eq!(
        agent_control_path_kind("agent/coder.d/life"),
        Some(AgentControlKind::Life)
    );
    assert_eq!(
        agent_control_path_kind("agent/rev-1.d/parent"),
        Some(AgentControlKind::Parent)
    );
    assert_eq!(agent_control_path_kind("agent/coder.d/label"), None);
    assert_eq!(agent_control_path_kind("model/openai/gpt-4o.d/session"), None);
    assert_eq!(agent_control_path_kind("agent/bad/name.d/uid"), None);
}
