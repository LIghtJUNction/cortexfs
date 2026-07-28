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
    assert!(ensure_reference_tree(&root).is_ok());

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
    assert!(ensure_reference_tree(&root).is_ok());
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
            "home/1000/model/openai/gpt-5.6.d/session/default",
            "shared/project-a/model/openai/gpt-5.6.d/session/default",
        ],
        is_durable_session_instance_path,
        true,
    );
    assert_path_matches(
        &[
            "home/1000/agent/coder/session",
            "home/1000/agent/coder/session/default/messages.jsonl",
            "shared/project-a/model/openai/gpt-5.6/session/default",
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
            "home/1000/model/openai/gpt-5.6.d/session/default/meta.json",
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
            "home/1000/model/openai/gpt-5.6.d/session/default/events.jsonl",
            "shared/project-a/model/openai/gpt-5.6.d/session/default/events.jsonl",
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
            "home/1000/model/openai/gpt-5.6.d/session/default/messages.jsonl",
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
            "home/1000/model/openai/gpt-5.6.d/session/default/context/swap/index.jsonl",
            Some(ContextJsonlKind::SwapIndex),
        ),
        (
            "shared/project-a/model/openai/gpt-5.6.d/session/default/context/dedup/index.jsonl",
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
            "model/openai/gpt-5.6",
            Some((ObjectClass::Model, "openai/gpt-5.6".to_owned())),
        ),
        ("agent/coder", Some((ObjectClass::Agent, "coder".to_owned()))),
        ("tool/fs.read", Some((ObjectClass::Tool, "fs.read".to_owned()))),
        ("tool/fs.read.d/schema", None),
        ("home/1000", None),
    ] {
        assert_eq!(
            parse_abi_path(path)
                .executable_object()
                .map(|(class, name)| (class, name.into_owned())),
            expected,
            "{path}"
        );
    }
}

#[test]
fn detects_model_capability_paths() {
    assert_path_matches(
        &["model/openai/gpt-5.6.d/cap", "model/google/gemini-3.6-flash.d/cap"],
        is_model_capability_path,
        true,
    );
    assert_path_matches(
        &["tool/fs.read.d/cap", "model/openai/gpt-5.6/cap", "model/openai/gpt-5.6.d/native"],
        is_model_capability_path,
        false,
    );
}

#[test]
fn detects_model_driver_paths() {
    assert_path_matches(
        &["model/openai/gpt-5.6.d/driver", "model/anthropic/claude-sonnet-5.d/driver"],
        is_model_driver_path,
        true,
    );
    assert_path_matches(
        &["model/openai/gpt-5.6/driver", "model/openai/gpt-5.6.d/cap"],
        is_model_driver_path,
        false,
    );
}

#[test]
fn detects_tool_schema_paths() {
    // Regression coverage for MCP placeholder schema path parsing semantics.
    assert_path_matches(
        &["tool/fs.read.d/schema", "tool/mcp.github.search_issues.d/schema"],
        is_tool_schema_path,
        true,
    );
    assert_path_matches(
        &["tool/fs.read/schema", "model/openai/gpt-5.6.d/schema", "tool/bad/name.d/schema"],
        is_tool_schema_path,
        false,
    );
}

#[test]
fn detects_shared_tool_schema_paths() {
    assert_path_matches(
        // Shared legacy placeholder schema path remains valid by parser grammar.
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
        ("model/openai/gpt-5.6.d/session", None),
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
