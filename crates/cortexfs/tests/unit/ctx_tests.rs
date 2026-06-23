use super::{
    agent_control_path_kind, context_jsonl_path_kind, doctor, executable_object_path, file_check,
    format_agent_control_issues, format_context_jsonl_issues, format_context_pack_issues,
    format_event_stream_issues, format_message_stream_issues, format_model_capability_issues,
    format_model_driver_route_error, format_object_layout_issues, format_session_control_issues,
    format_session_index_issues, format_session_layout_issues, format_shared_queue_layout_issues,
    format_tool_schema_issues, is_context_pack_path, is_durable_session_instance_path,
    is_model_capability_path, is_model_driver_path, is_session_events_path,
    is_session_messages_path, is_shared_queue_root_path, is_shared_tool_schema_path,
    is_tool_schema_path, json_string, list_names, newline_terminated, parse_command,
    resolve_abi_path, session_control_path_kind, session_index_path_kind, stream_socket_request,
    Command, FileCommand, LsTarget, ObjectClass, MAX_SOCKET_FRAME_BYTES,
};
use cortexfs::{
    ensure_v1_reference_tree, AgentControlIssue, AgentControlKind, ContextJsonlIssue,
    ContextJsonlKind, ContextPackIssue, ContextPackSourceError, EventStreamIssue,
    MessageStreamIssue, ModelCapabilityIssue, ModelDriverRouteError, ObjectLayoutIssue,
    SessionControlIssue, SessionControlKind, SessionIndexIssue, SessionIndexKind,
    SessionLayoutIssue, SharedQueueLayoutIssue, ToolSchemaIssue, CHILD_RESULT_REQUIRED_FILES,
    CONTEXT_REQUIRED_DIRS, CONTEXT_REQUIRED_FILES, SESSION_REQUIRED_FILES,
};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

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

#[test]
fn formats_session_layout_issues_for_file_check() {
    let formatted = format_session_layout_issues(&[
        SessionLayoutIssue::MissingFile("messages.jsonl".to_owned()),
        SessionLayoutIssue::NotDirectory("context".to_owned()),
        SessionLayoutIssue::InvalidFileValue {
            path: "state".to_owned(),
            value: "running".to_owned(),
        },
    ]);
    assert_eq!(
        formatted,
        "missing file messages.jsonl, not directory context, invalid file value state=running"
    );
}

#[test]
fn formats_context_pack_issues_for_file_check() {
    let formatted = format_context_pack_issues(&[
        ContextPackIssue::InvalidSource {
            item: 1,
            source: "../other/messages.jsonl".to_owned(),
            reason: ContextPackSourceError::ParentComponent,
        },
        ContextPackIssue::MissingSource(2),
        ContextPackIssue::InvalidJson,
    ]);
    assert_eq!(
            formatted,
            "invalid source item 1 ../other/messages.jsonl (parent component), missing source item 2, invalid json"
        );
}

#[test]
fn formats_event_stream_issues_for_file_check() {
    let formatted = format_event_stream_issues(&[
        EventStreamIssue::ProviderNativeField {
            line: 1,
            field: "response_id".to_owned(),
        },
        EventStreamIssue::UnknownType {
            line: 2,
            event_type: "native_thread".to_owned(),
        },
        EventStreamIssue::InvalidUsage(3),
        EventStreamIssue::InvalidAgentLifecycle(4),
    ]);
    assert_eq!(
            formatted,
            "provider native field line 1 response_id, unknown type line 2 native_thread, invalid usage line 3, invalid agent lifecycle line 4"
        );
}

#[test]
fn formats_message_stream_issues_for_file_check() {
    let formatted = format_message_stream_issues(&[
        MessageStreamIssue::ProviderNativeField {
            line: 1,
            field: "thread_id".to_owned(),
        },
        MessageStreamIssue::InvalidRole {
            line: 2,
            role: "developer".to_owned(),
        },
        MessageStreamIssue::InvalidContent(3),
        MessageStreamIssue::MissingContent(4),
    ]);
    assert_eq!(
            formatted,
            "provider native field line 1 thread_id, invalid role line 2 developer, invalid content line 3, missing content line 4"
        );
}

#[test]
fn formats_context_jsonl_issues_for_file_check() {
    let formatted = format_context_jsonl_issues(&[
        ContextJsonlIssue::InvalidField {
            line: 1,
            field: "path".to_owned(),
            value: "../secret".to_owned(),
        },
        ContextJsonlIssue::MissingStringField {
            line: 2,
            field: "source".to_owned(),
        },
        ContextJsonlIssue::MissingNumberField {
            line: 3,
            field: "tokens".to_owned(),
        },
        ContextJsonlIssue::MissingStringArrayField {
            line: 4,
            field: "refs".to_owned(),
        },
    ]);
    assert_eq!(
            formatted,
            "invalid field line 1 path=../secret, missing string field line 2 source, missing number field line 3 tokens, missing string array field line 4 refs"
        );
}

#[test]
fn formats_model_capability_issues_for_file_check() {
    let formatted = format_model_capability_issues(&[
        ModelCapabilityIssue::ProviderPrivate {
            line: 1,
            capability: "openai_responses".to_owned(),
        },
        ModelCapabilityIssue::Unknown {
            line: 2,
            capability: "vendor_magic".to_owned(),
        },
    ]);
    assert_eq!(
            formatted,
            "provider private capability line 1 openai_responses, unknown capability line 2 vendor_magic"
        );
}

#[test]
fn formats_model_driver_route_errors_for_file_check() {
    assert_eq!(
        format_model_driver_route_error(&ModelDriverRouteError::UnknownUseCase {
            line: 2,
            value: "direct".to_owned()
        }),
        "unknown driver use case line 2 direct"
    );
    assert_eq!(
        format_model_driver_route_error(&ModelDriverRouteError::InvalidDriverName {
            line: 1,
            value: "/bin/sh".to_owned()
        }),
        "invalid driver name line 1 /bin/sh"
    );
}

#[test]
fn formats_tool_schema_issues_for_file_check() {
    let formatted = format_tool_schema_issues(&[
        ToolSchemaIssue::AuthorityField("policy".to_owned()),
        ToolSchemaIssue::InvalidJson,
        ToolSchemaIssue::NotObject,
    ]);
    assert_eq!(
        formatted,
        "authority field policy, invalid json, not object"
    );
}

#[test]
fn formats_session_index_issues_for_file_check() {
    let formatted = format_session_index_issues(&[
        SessionIndexIssue::InvalidSessionName {
            line: 2,
            value: "bad/name".to_owned(),
        },
        SessionIndexIssue::MultipleValues { line: 3 },
        SessionIndexIssue::EmptyValue { line: 4 },
    ]);
    assert_eq!(
        formatted,
        "invalid session name line 2 bad/name, multiple values line 3, empty value line 4"
    );
}

#[test]
fn formats_agent_control_issues_for_file_check() {
    let formatted = format_agent_control_issues(&[
        AgentControlIssue::InvalidNumber {
            line: 1,
            value: "abc".to_owned(),
        },
        AgentControlIssue::InvalidValue {
            line: 2,
            value: "detached".to_owned(),
        },
        AgentControlIssue::MultipleValues { line: 3 },
        AgentControlIssue::EmptyValue,
    ]);
    assert_eq!(
            formatted,
            "invalid number line 1 abc, invalid value line 2 detached, multiple values line 3, empty value"
        );
}

#[test]
fn formats_session_control_issues_for_file_check() {
    let formatted = format_session_control_issues(&[
        SessionControlIssue::InvalidValue {
            line: 1,
            value: "running".to_owned(),
        },
        SessionControlIssue::MultipleValues { line: 2 },
        SessionControlIssue::InvalidJson,
        SessionControlIssue::NotObject,
        SessionControlIssue::EmptyValue,
    ]);
    assert_eq!(
            formatted,
            "invalid value line 1 running, multiple values line 2, invalid json, not object, empty value"
        );
}

#[test]
fn file_check_validates_session_control_files() {
    let root = unique_test_dir("ctx-session-control-check");
    let session = root
        .join("home")
        .join("1000")
        .join("agent")
        .join("coder")
        .join("session")
        .join("default");
    assert!(fs::create_dir_all(&session).is_ok());
    assert!(fs::write(session.join("state"), "idle\n").is_ok());
    assert!(fs::write(session.join("cwd"), "/work\n").is_ok());
    assert!(fs::write(
        session.join("meta.json"),
        "{\"client\":\"ctx\",\"model\":\"openai/gpt-4o\",\"scope\":\"private\"}\n"
    )
    .is_ok());

    assert!(file_check(&root, "home/1000/agent/coder/session/default/state").is_ok());
    assert!(file_check(&root, "home/1000/agent/coder/session/default/cwd").is_ok());
    assert!(file_check(&root, "home/1000/agent/coder/session/default/meta.json").is_ok());

    assert!(fs::write(session.join("cwd"), "../host\n").is_ok());
    let checked = file_check(&root, "home/1000/agent/coder/session/default/cwd");
    assert!(
        matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("invalid value"))
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn file_check_validates_agent_control_files() {
    let root = unique_test_dir("ctx-agent-control-check");
    let control = root.join("agent").join("coder.d");
    assert!(fs::create_dir_all(&control).is_ok());
    assert!(fs::write(control.join("uid"), "1000\n").is_ok());
    assert!(fs::write(control.join("life"), "detached\n").is_ok());
    assert!(fs::write(
        control.join("parent"),
        "agent:coder session:default run:r1\n"
    )
    .is_ok());

    assert!(file_check(&root, "agent/coder.d/uid").is_ok());
    let checked = file_check(&root, "agent/coder.d/life");
    assert!(
        matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("invalid value"))
    );
    assert!(file_check(&root, "agent/coder.d/parent").is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn file_check_validates_session_index_files() {
    let root = unique_test_dir("ctx-session-index-check");
    let index = root
        .join("shared")
        .join("im-qq-dev")
        .join("agent")
        .join("bot")
        .join("session")
        .join("index");
    assert!(fs::create_dir_all(index.join("by-cwd")).is_ok());
    assert!(fs::write(index.join("list"), "group-456\nbad/name\n").is_ok());
    assert!(fs::write(index.join("current"), "group-456\n").is_ok());
    assert!(fs::write(index.join("by-cwd").join("hash-1"), "group-456\n").is_ok());

    let checked = file_check(&root, "shared/im-qq-dev/agent/bot/session/index/list");
    assert!(
        matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("invalid session name"))
    );
    assert!(file_check(&root, "shared/im-qq-dev/agent/bot/session/index/current").is_ok());
    assert!(file_check(
        &root,
        "shared/im-qq-dev/agent/bot/session/index/by-cwd/hash-1"
    )
    .is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn file_check_rejects_by_cwd_symlink_index_entries() {
    let root = unique_test_dir("ctx-session-index-symlink");
    let by_cwd = root
        .join("home")
        .join("1000")
        .join("agent")
        .join("coder")
        .join("session")
        .join("index")
        .join("by-cwd");
    assert!(fs::create_dir_all(&by_cwd).is_ok());
    assert!(fs::write(by_cwd.join("target"), "default\n").is_ok());
    assert!(std::os::unix::fs::symlink("target", by_cwd.join("hash-1")).is_ok());

    let checked = file_check(&root, "home/1000/agent/coder/session/index/by-cwd/hash-1");
    assert!(
        matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("by-cwd entry is a symlink"))
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn file_check_validates_model_capability_files() {
    let root = unique_test_dir("ctx-model-cap-check");
    let cap = root.join("model").join("openai").join("gpt-4o.d").join("cap");
    let parent = cap.parent();
    assert!(parent.is_some());
    let Some(parent) = parent else { return };
    assert!(fs::create_dir_all(parent).is_ok());
    assert!(fs::write(&cap, "chat\nopenai_responses\n").is_ok());

    let checked = file_check(&root, "model/openai/gpt-4o.d/cap");
    assert!(
        matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("provider private capability"))
    );

    assert!(fs::write(&cap, "chat\nstream\n").is_ok());
    let checked = file_check(&root, "model/openai/gpt-4o.d/cap");
    assert!(checked.is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn file_check_validates_model_driver_route_files() {
    let root = unique_test_dir("ctx-model-driver-check");
    let driver = root
        .join("model")
        .join("openai")
        .join("gpt-4o.d")
        .join("driver");
    let parent = driver.parent();
    assert!(parent.is_some());
    let Some(parent) = parent else { return };
    assert!(fs::create_dir_all(parent).is_ok());
    assert!(fs::write(&driver, "agent=/bin/sh\n").is_ok());

    let checked = file_check(&root, "model/openai/gpt-4o.d/driver");
    assert!(
        matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("invalid driver name"))
    );

    assert!(fs::write(
        &driver,
        "default=openai-chat\nexec=openai-chat\nagent=openai-responses,openai-chat\n"
    )
    .is_ok());
    let checked = file_check(&root, "model/openai/gpt-4o.d/driver");
    assert!(checked.is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn file_check_validates_tool_schema_files() {
    let root = unique_test_dir("ctx-tool-schema-check");
    let schema = root.join("tool").join("fs.read.d").join("schema");
    let parent = schema.parent();
    assert!(parent.is_some());
    let Some(parent) = parent else { return };
    assert!(fs::create_dir_all(parent).is_ok());
    assert!(fs::write(
        &schema,
        "{\"type\":\"object\",\"properties\":{\"path\":{\"type\":\"string\"}}}\n"
    )
    .is_ok());
    assert!(file_check(&root, "tool/fs.read.d/schema").is_ok());

    assert!(fs::write(&schema, "{\"policy\":\"allow all\"}\n").is_ok());
    let checked = file_check(&root, "tool/fs.read.d/schema");
    assert!(
        matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("authority field policy"))
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn file_check_validates_shared_tool_schema_files() {
    let root = unique_test_dir("ctx-shared-tool-schema-check");
    let schema = root
        .join("shared")
        .join("project-a")
        .join("tool")
        .join("project.test.d")
        .join("schema");
    let parent = schema.parent();
    assert!(parent.is_some());
    let Some(parent) = parent else { return };
    assert!(fs::create_dir_all(parent).is_ok());
    assert!(fs::write(
        &schema,
        "{\"type\":\"object\",\"properties\":{\"target\":{\"type\":\"string\"}}}\n"
    )
    .is_ok());
    assert!(file_check(&root, "shared/project-a/tool/project.test.d/schema").is_ok());

    assert!(fs::write(&schema, "{\"authority\":\"local\"}\n").is_ok());
    let checked = file_check(&root, "shared/project-a/tool/project.test.d/schema");
    assert!(
        matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("invalid tool schema") && error.message.contains("authority field authority"))
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn file_check_validates_shared_queue_roots() {
    let root = unique_test_dir("ctx-shared-queue-check");
    let queue = root.join("shared").join("project-a").join("queue");
    for name in ["inbox", "pending", "lease", "claimed", "done", "failed"] {
        assert!(fs::create_dir_all(queue.join(name)).is_ok());
    }

    assert!(file_check(&root, "shared/project-a/queue").is_ok());

    assert!(fs::remove_dir(queue.join("lease")).is_ok());
    let checked = file_check(&root, "shared/project-a/queue");
    assert!(
        matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("invalid shared queue") && error.message.contains("missing directory lease"))
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn file_check_validates_event_stream_files() {
    let root = unique_test_dir("ctx-events-check");
    let events = root
        .join("home")
        .join("1000")
        .join("agent")
        .join("coder")
        .join("session")
        .join("default")
        .join("events.jsonl");
    let parent = events.parent();
    assert!(parent.is_some());
    let Some(parent) = parent else { return };
    assert!(fs::create_dir_all(parent).is_ok());
    assert!(fs::write(
        &events,
        "{\"type\":\"start\",\"run\":\"r1\",\"response_id\":\"resp_1\"}\n"
    )
    .is_ok());

    let checked = file_check(&root, "home/1000/agent/coder/session/default/events.jsonl");
    assert!(
        matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("provider native field"))
    );

    let model_events = root
        .join("shared")
        .join("project-a")
        .join("model")
        .join("openai")
        .join("gpt-4o.d")
        .join("session")
        .join("default")
        .join("events.jsonl");
    let parent = model_events.parent();
    assert!(parent.is_some());
    let Some(parent) = parent else { return };
    assert!(fs::create_dir_all(parent).is_ok());
    assert!(fs::write(
        &model_events,
        "{\"type\":\"done\",\"run\":\"r1\",\"status\":\"ok\"}\n"
    )
    .is_ok());
    assert!(file_check(
        &root,
        "shared/project-a/model/openai/gpt-4o.d/session/default/events.jsonl"
    )
    .is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn file_check_validates_message_stream_files() {
    let root = unique_test_dir("ctx-messages-check");
    let messages = root
        .join("home")
        .join("1000")
        .join("agent")
        .join("coder")
        .join("session")
        .join("default")
        .join("messages.jsonl");
    let parent = messages.parent();
    assert!(parent.is_some());
    let Some(parent) = parent else { return };
    assert!(fs::create_dir_all(parent).is_ok());
    assert!(fs::write(
        &messages,
        "{\"role\":\"assistant\",\"response_id\":\"resp_1\",\"content\":\"hello\"}\n"
    )
    .is_ok());

    let checked = file_check(
        &root,
        "home/1000/agent/coder/session/default/messages.jsonl",
    );
    assert!(
        matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("provider native field"))
    );

    assert!(fs::write(
        &messages,
        "{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}\n"
    )
    .is_ok());
    assert!(file_check(
        &root,
        "home/1000/agent/coder/session/default/messages.jsonl"
    )
    .is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn file_check_validates_context_jsonl_files() {
    let root = unique_test_dir("ctx-context-jsonl-check");
    let context = root
        .join("shared")
        .join("project-a")
        .join("agent")
        .join("coder")
        .join("session")
        .join("default")
        .join("context");
    assert!(fs::create_dir_all(context.join("swap")).is_ok());
    assert!(fs::write(
        context.join("facts.jsonl"),
        "{\"id\":\"f1\",\"text\":\"root is frozen\",\"source\":\"messages:1-2\"}\n"
    )
    .is_ok());
    assert!(
            fs::write(
                context.join("swap").join("index.jsonl"),
                "{\"id\":\"sha256-abc\",\"kind\":\"message_range\",\"source\":\"provider_thread\",\"summary\":\"bad\",\"tokens\":\"10\"}\n"
            )
            .is_ok()
        );

    assert!(file_check(
        &root,
        "shared/project-a/agent/coder/session/default/context/facts.jsonl"
    )
    .is_ok());
    let checked = file_check(
        &root,
        "shared/project-a/agent/coder/session/default/context/swap/index.jsonl",
    );
    assert!(
        matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("invalid context jsonl"))
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn file_check_validates_shared_and_model_session_layouts() {
    let root = unique_test_dir("ctx-shared-model-session-check");
    let shared_agent = root
        .join("shared")
        .join("im-qq-dev")
        .join("agent")
        .join("bot")
        .join("session")
        .join("group-456");
    let model_session = root
        .join("home")
        .join("1000")
        .join("model")
        .join("openai")
        .join("gpt-4o.d")
        .join("session")
        .join("default");
    create_complete_session_layout(&shared_agent);
    create_complete_session_layout(&model_session);

    assert!(file_check(&root, "shared/im-qq-dev/agent/bot/session/group-456").is_ok());
    assert!(file_check(
        &root,
        "home/1000/model/openai/gpt-4o.d/session/default"
    )
    .is_ok());

    assert!(fs::remove_file(model_session.join("messages.jsonl")).is_ok());
    let checked = file_check(
        &root,
        "home/1000/model/openai/gpt-4o.d/session/default",
    );
    assert!(
        matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("missing file messages.jsonl"))
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn doctor_validates_reference_tree_objects_sessions_and_queue() {
    let root = unique_test_dir("ctx-doctor-reference-tree");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    let ensured = ensure_v1_reference_tree(&root);
    assert!(ensured.is_ok());

    assert!(doctor(&root).is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn doctor_reports_reference_tree_layout_breakage() {
    let root = unique_test_dir("ctx-doctor-reference-tree-bad");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    let ensured = ensure_v1_reference_tree(&root);
    assert!(ensured.is_ok());
    assert!(fs::remove_file(root.join("tool").join("fs.read.d").join("schema")).is_ok());
    assert!(fs::remove_dir_all(
        root.join("home")
            .join("1000")
            .join("agent")
            .join("coder")
            .join("session")
            .join("index")
            .join("by-cwd")
    )
    .is_ok());
    let checked = doctor(&root);
    assert!(matches!(
        checked,
        Err(ref error) if error.code == 69 && error.message.contains("doctor found ABI problems")
    ));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn formats_shared_queue_layout_issues_for_doctor() {
    let formatted = format_shared_queue_layout_issues(&[
        SharedQueueLayoutIssue::MissingDirectory("done".to_owned()),
        SharedQueueLayoutIssue::NotDirectory("failed".to_owned()),
    ]);
    assert_eq!(formatted, "missing directory done, not directory failed");
}

#[test]
fn formats_object_layout_issues_for_file_check() {
    let formatted = format_object_layout_issues(&[
        ObjectLayoutIssue::MissingExecutable("agent/coder".to_owned()),
        ObjectLayoutIssue::InvalidControlValue {
            path: "model/openai/gpt-4o.d/session".to_owned(),
            value: "native_thread".to_owned(),
        },
    ]);
    assert_eq!(
        formatted,
        "missing executable agent/coder, invalid control value model/openai/gpt-4o.d/session=native_thread"
    );
}

#[test]
fn control_file_values_end_in_newline() {
    assert_eq!(newline_terminated("cwd=/work"), "cwd=/work\n");
    assert_eq!(newline_terminated("cwd=/work\n"), "cwd=/work\n");
}

#[test]
fn json_strings_escape_socket_request_values() {
    assert_eq!(json_string("default"), "\"default\"");
    assert_eq!(json_string("quote\"slash\\"), "\"quote\\\"slash\\\\\"");
    assert_eq!(json_string("line\nnext"), "\"line\\nnext\"");
}

#[test]
fn socket_requests_enforce_frame_limit_before_connecting() {
    let request = "x".repeat(MAX_SOCKET_FRAME_BYTES + 1);
    let result = stream_socket_request(Path::new("/does/not/exist.sock"), &request);
    assert!(
        matches!(result, Err(ref error) if error.code == 2 && error.message.contains("EMSGSIZE"))
    );
}

fn unique_test_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!(
        "cortexfs-ctx-{name}-{}-{nanos}",
        std::process::id()
    ))
}

fn create_complete_session_layout(session: &Path) {
    let context = session.join("context");
    assert!(fs::create_dir_all(&context).is_ok());
    for file in SESSION_REQUIRED_FILES {
        write_text_file(&session.join(file), session_file_fixture_value(file));
    }
    for file in CONTEXT_REQUIRED_FILES {
        write_text_file(&context.join(file), "ok\n");
    }
    for dir in CONTEXT_REQUIRED_DIRS {
        assert!(fs::create_dir_all(context.join(dir)).is_ok());
    }
    let child = context.join("child").join("rev-1");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    for file in CHILD_RESULT_REQUIRED_FILES {
        write_text_file(&child.join(file), "ok\n");
    }
}

fn session_file_fixture_value(file: &str) -> &'static str {
    match file {
        "state" => "idle\n",
        "cwd" => "/work\n",
        "meta.json" => "{\"client\":\"ctx\",\"model\":\"openai/gpt-4o\",\"scope\":\"private\"}\n",
        _ => "ok\n",
    }
}

fn write_text_file(path: &Path, content: &str) {
    let Some(parent) = path.parent() else {
        return;
    };
    assert!(fs::create_dir_all(parent).is_ok());
    assert!(fs::write(path, content).is_ok());
}
