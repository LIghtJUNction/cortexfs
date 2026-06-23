#[test]
fn parses_spec_which_command() {
    let command = cmd!("which", "tool", "fs.read");
    assert!(matches!(
        command,
        Ok(Command::Which(ObjectClass::Tool, ref name)) if name == "fs.read"
    ));
}

#[test]
fn parses_file_set_command() {
    let command = cmd!("file", "set", "agent/coder.d/cwd", "/work");
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
    let explicit = cmd!("file", "classify", "tool/fs.read");
    assert!(matches!(
        explicit,
        Ok(Command::File(ref args))
            if args.command == FileCommand::Classify
                && args.path == "tool/fs.read"
                && args.value.is_none()
    ));

    let shorthand = cmd!("file", "tool/fs.read");
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

    let latest = cmd!("latest", "coder", "default");
    assert!(matches!(
        latest,
        Ok(Command::Latest {
            ref agent,
            session: Some(ref session)
        }) if agent == "coder" && session == "default"
    ));

    let resume = cmd!("resume", "coder", "default");
    assert!(matches!(
        resume,
        Ok(Command::Resume {
            ref agent,
            session: Some(ref session)
        }) if agent == "coder" && session == "default"
    ));

    let send = cmd!("send", "coder", "default", "hello");
    assert!(matches!(
        send,
        Ok(Command::Send {
            ref agent,
            ref session,
            ref input
        }) if agent == "coder" && session == "default" && input == "hello"
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
fn parses_bootstrap_and_mount_commands() {
    let bootstrap = cmd!("bootstrap");
    assert!(matches!(bootstrap, Ok(Command::Bootstrap { source: None })));

    let bootstrap_source = cmd!("bootstrap", "/tmp/cortexfs-source");
    assert!(matches!(
        bootstrap_source,
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

    let tool = list_names(&root, &LsTarget::Path("tool".to_owned()));
    assert!(matches!(
        tool,
        Ok(ref names)
            if names.contains(&"fs.read".to_owned())
                && !names.contains(&"fs.read.d".to_owned())
    ));
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
