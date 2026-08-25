#[test]
fn formats_session_layout_issues_for_file_check() {
    assert_eq!(
        format_session_layout_issues(&[
            PathLayoutIssue::missing("messages.jsonl".to_owned(), LayoutPathRole::File),
            PathLayoutIssue::wrong_kind("context".to_owned(), LayoutPathRole::Directory),
            PathLayoutIssue::invalid_value("state".to_owned(), "running".to_owned()),
        ]),
        "missing file messages.jsonl, not directory context, invalid value state=running"
    );
}

#[test]
fn formats_context_pack_issues_for_file_check() {
    assert_eq!(
        format_context_pack_issues(&[
            ContextPackIssue::InvalidSource {
                item: 1,
                source: "../other/messages.jsonl".to_owned(),
                reason: ContextPackSourceError::ParentComponent,
            },
            ContextPackIssue::MissingSource(2),
            ContextPackIssue::InvalidJson,
        ]),
        "invalid source item 1 ../other/messages.jsonl (parent component), missing source item 2, invalid json"
    );
}

#[test]
fn formats_agent_schedule_issues_for_file_check() {
    let rendered = format_agent_schedule_issues(&[
        AgentScheduleIssue::InvalidReactBound {
            node: "review".to_owned(),
        },
        AgentScheduleIssue::PermissionNotGranted {
            node: "fetch\u{1b}[31m".to_owned(),
            class: "tool".to_owned(),
            name: "fs.read".to_owned(),
            permission: "execute".to_owned(),
        },
    ]);

    assert_eq!(
        rendered,
        "invalid react bound node review, permission not granted node fetch\\u{1b}[31m tool:fs.read execute"
    );
    assert!(!rendered.as_bytes().contains(&0x1b));
}

#[test]
fn file_check_formatters_escape_terminal_controls() {
    let rendered = format_message_stream_issues(&[
        MessageStreamIssue::ProviderNativeField {
            line: 1,
            field: "thread\u{1b}]52;c;payload\u{7}".to_owned(),
        },
        MessageStreamIssue::InvalidRole {
            line: 2,
            role: "developer\u{1b}[31m".to_owned(),
        },
    ]);

    assert_eq!(
        rendered,
        "provider native field line 1 thread\\u{1b}]52;c;payload\\u{7}, invalid role line 2 developer\\u{1b}[31m"
    );
    assert!(!rendered.as_bytes().contains(&0x1b));
    assert!(!rendered.as_bytes().contains(&0x07));
}

#[test]
fn formats_event_stream_issues_for_file_check() {
    assert_eq!(
        format_event_stream_issues(&[
            EventStreamIssue::MissingFinalNewline(1),
            EventStreamIssue::ProviderNativeField {
                line: 1,
                field: "response_id".to_owned(),
            },
            EventStreamIssue::UnknownType {
                line: 2,
                event_type: "native_thread".to_owned(),
            },
            EventStreamIssue::InvalidUsage(3),
            EventStreamIssue::InvalidApproval(4),
            EventStreamIssue::InvalidAgentLifecycle(5),
        ]),
        "missing final newline line 1, provider native field line 1 response_id, unknown type line 2 native_thread, invalid usage line 3, invalid approval line 4, invalid agent lifecycle line 5"
    );
}

#[test]
fn formats_message_stream_issues_for_file_check() {
    assert_eq!(
        format_message_stream_issues(&[
            MessageStreamIssue::MissingFinalNewline(1),
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
        ]),
        "missing final newline line 1, provider native field line 1 thread_id, invalid role line 2 developer, invalid content line 3, missing content line 4"
    );
}

#[test]
fn formats_context_jsonl_issues_for_file_check() {
    assert_eq!(
        format_context_jsonl_issues(&[
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
        ]),
        "invalid field line 1 path=../secret, missing string field line 2 source, missing number field line 3 tokens, missing string array field line 4 refs"
    );
}

#[test]
fn formats_model_capability_issues_for_file_check() {
    assert_eq!(
        format_model_capability_issues(&[
            ModelCapabilityIssue::ProviderPrivate {
                line: 1,
                capability: "openai_responses".to_owned(),
            },
            ModelCapabilityIssue::Unknown {
                line: 2,
                capability: "vendor_magic".to_owned(),
            },
        ]),
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
    assert_eq!(
        format_tool_schema_issues(&[
            ToolSchemaIssue::AuthorityField("policy".to_owned()),
            ToolSchemaIssue::InvalidJson,
            ToolSchemaIssue::NotObject,
        ]),
        "authority field policy, invalid json, not object"
    );
}

#[test]
fn formats_session_index_issues_for_file_check() {
    assert_eq!(
        format_session_index_issues(&[
            ControlLineIssue::InvalidValue {
                line: 2,
                value: "bad/name".to_owned(),
            },
            ControlLineIssue::MultipleValues { line: 3 },
            ControlLineIssue::EmptyValue { line: 4 },
        ]),
        "invalid value line 2 bad/name, multiple values line 3, empty value line 4"
    );
}

#[test]
fn formats_agent_control_issues_for_file_check() {
    assert_eq!(
        format_agent_control_issues(&[
            ControlLineIssue::InvalidNumber {
                line: 1,
                value: "abc".to_owned(),
            },
            ControlLineIssue::InvalidValue {
                line: 2,
                value: "detached".to_owned(),
            },
            ControlLineIssue::MultipleValues { line: 3 },
            ControlLineIssue::EmptyValue { line: 1 },
        ]),
        "invalid number line 1 abc, invalid value line 2 detached, multiple values line 3, empty value line 1"
    );
}

#[test]
fn formats_session_control_issues_for_file_check() {
    assert_eq!(
        format_session_control_issues(&[
            ControlLineIssue::InvalidValue {
                line: 1,
                value: "running".to_owned(),
            },
            ControlLineIssue::MultipleValues { line: 2 },
            ControlLineIssue::InvalidJson,
            ControlLineIssue::NotObject,
            ControlLineIssue::EmptyValue { line: 1 },
        ]),
        "invalid value line 1 running, multiple values line 2, invalid json, not object, empty value line 1"
    );
}

#[test]
fn file_check_validates_session_control_files() {
    let root = clean_test_dir("ctx-session-control-check");
    let session = fixture_path(&root, &["home", "1000", "agent", "executor", "session", "default"]);
    assert!(fs::create_dir_all(&session).is_ok());
    assert!(fs::write(session.join("state"), "idle\n").is_ok());
    assert!(fs::write(session.join("cwd"), "/work\n").is_ok());
    assert!(fs::write(
        session.join("meta.json"),
        "{\"client\":\"ctx\",\"model\":\"openai/gpt-5.6\",\"scope\":\"private\"}\n"
    )
    .is_ok());

    assert!(file_check(&root, "home/1000/agent/executor/session/default/state").is_ok());
    assert!(file_check(&root, "home/1000/agent/executor/session/default/cwd").is_ok());
    assert!(file_check(&root, "home/1000/agent/executor/session/default/meta.json").is_ok());

    assert!(fs::write(session.join("cwd"), "../host\n").is_ok());
    assert_file_check_error_contains(
        &root,
        "home/1000/agent/executor/session/default/cwd",
        &["invalid value"],
    );
}

#[test]
fn file_check_validates_agent_control_files() {
    let root = clean_test_dir("ctx-agent-control-check");
    let control = root.join("agent").join("executor.d");
    assert!(fs::create_dir_all(&control).is_ok());
    assert!(fs::write(control.join("uid"), "1000\n").is_ok());
    assert!(fs::write(control.join("life"), "detached\n").is_ok());
    assert!(fs::write(
        control.join("parent"),
        "agent:executor session:default run:r1\n"
    )
    .is_ok());

    assert!(file_check(&root, "agent/executor.d/uid").is_ok());
    assert_file_check_error_contains(&root, "agent/executor.d/life", &["invalid value"]);
    assert!(file_check(&root, "agent/executor.d/parent").is_ok());
}

#[test]
fn file_check_validates_session_index_files() {
    let root = clean_test_dir("ctx-session-index-check");
    let index = fixture_path(
        &root,
        &["shared", "im-qq-dev", "agent", "bot", "session", "index"],
    );
    assert!(fs::create_dir_all(index.join("by-cwd")).is_ok());
    assert!(fs::create_dir_all(index.join("by-hash")).is_ok());
    assert!(fs::create_dir_all(index.join("by-uuid")).is_ok());
    assert!(fs::write(index.join("list"), "group-456\nbad/name\n").is_ok());
    assert!(fs::write(index.join("current"), "group-456\n").is_ok());
    assert!(fs::write(index.join("by-cwd").join("hash-1"), "group-456\n").is_ok());
    assert!(fs::write(index.join("by-hash").join("hash-1"), "group-456\n").is_ok());
    assert!(fs::write(index.join("by-uuid").join("uuid-1"), "group-456\n").is_ok());

    assert_file_check_error_contains(
        &root,
        "shared/im-qq-dev/agent/bot/session/index/list",
        &["invalid value"],
    );
    assert!(file_check(&root, "shared/im-qq-dev/agent/bot/session/index/current").is_ok());
    assert!(file_check(
        &root,
        "shared/im-qq-dev/agent/bot/session/index/by-cwd/hash-1"
    )
    .is_ok());
    assert!(file_check(
        &root,
        "shared/im-qq-dev/agent/bot/session/index/by-hash/hash-1"
    )
    .is_ok());
    assert!(file_check(
        &root,
        "shared/im-qq-dev/agent/bot/session/index/by-uuid/uuid-1"
    )
    .is_ok());
}

#[test]
fn file_check_rejects_by_cwd_symlink_index_entries() {
    let root = clean_test_dir("ctx-session-index-symlink");
    let by_cwd = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "executor", "session", "index", "by-hash",
        ],
    );
    assert!(fs::create_dir_all(&by_cwd).is_ok());
    assert!(fs::write(by_cwd.join("target"), "default\n").is_ok());
    assert!(std::os::unix::fs::symlink("target", by_cwd.join("hash-1")).is_ok());

    assert_file_check_error_contains(
        &root,
        "home/1000/agent/executor/session/index/by-hash/hash-1",
        &["secondary index entry is a symlink"],
    );
}

#[test]
fn file_check_validates_model_capability_files() {
    let root = clean_test_dir("ctx-model-cap-check");
    let cap = fixture_path(&root, &["model", "openai", "gpt-5.6.d", "cap"]);
    write_text_file(&cap, "chat\nopenai_responses\n");

    assert_file_check_error_contains(
        &root,
        "model/openai/gpt-5.6.d/cap",
        &["provider private capability"],
    );

    write_text_file(&cap, "chat\nstream\n");
    let checked = file_check(&root, "model/openai/gpt-5.6.d/cap");
    assert!(checked.is_ok());
}

#[test]
fn file_check_validates_model_driver_route_files() {
    let root = clean_test_dir("ctx-model-driver-check");
    let driver = fixture_path(&root, &["model", "openai", "gpt-5.6.d", "driver"]);
    write_text_file(&driver, "agent=/bin/sh\n");

    assert_file_check_error_contains(
        &root,
        "model/openai/gpt-5.6.d/driver",
        &["invalid driver name"],
    );

    write_text_file(
        &driver,
        "default=openai-chat\nexec=openai-chat\nagent=openai-responses,openai-chat\n"
    );
    let checked = file_check(&root, "model/openai/gpt-5.6.d/driver");
    assert!(checked.is_ok());
}

#[test]
fn file_check_validates_tool_schema_files() {
    let root = clean_test_dir("ctx-tool-schema-check");
    let schema = root.join("tool").join("fs.read.d").join("schema");
    write_text_file(
        &schema,
        "{\"type\":\"object\",\"properties\":{\"path\":{\"type\":\"string\"}}}\n"
    );
    assert!(file_check(&root, "tool/fs.read.d/schema").is_ok());

    write_text_file(&schema, "{\"policy\":\"allow all\"}\n");
    assert_file_check_error_contains(&root, "tool/fs.read.d/schema", &["authority field policy"]);
}

#[test]
fn file_check_validates_shared_tool_schema_files() {
    let root = clean_test_dir("ctx-shared-tool-schema-check");
    let schema = fixture_path(
        &root,
        &["shared", "project-a", "tool", "project.test.d", "schema"],
    );
    write_text_file(
        &schema,
        "{\"type\":\"object\",\"properties\":{\"target\":{\"type\":\"string\"}}}\n"
    );
    assert!(file_check(&root, "shared/project-a/tool/project.test.d/schema").is_ok());

    write_text_file(&schema, "{\"authority\":\"local\"}\n");
    assert_file_check_error_contains(
        &root,
        "shared/project-a/tool/project.test.d/schema",
        &["invalid tool schema", "authority field authority"],
    );
}

#[test]
fn file_check_validates_shared_queue_roots() {
    let root = clean_test_dir("ctx-shared-queue-check");
    let queue = root.join("shared").join("project-a").join("queue");
    for name in ["inbox", "pending", "lease", "claimed", "done", "failed"] {
        assert!(fs::create_dir_all(queue.join(name)).is_ok());
    }

    assert!(file_check(&root, "shared/project-a/queue").is_ok());

    assert!(fs::remove_dir(queue.join("lease")).is_ok());
    assert_file_check_error_contains(
        &root,
        "shared/project-a/queue",
        &["invalid shared queue", "missing directory lease"],
    );
}

#[test]
fn file_check_validates_event_stream_files() {
    let root = clean_test_dir("ctx-events-check");
    let events = fixture_path(
        &root,
        &[
            "home",
            "1000",
            "agent",
            "executor",
            "session",
            "default",
            "events.jsonl",
        ],
    );
    write_text_file(
        &events,
        "{\"type\":\"start\",\"run\":\"r1\",\"response_id\":\"resp_1\"}\n"
    );
    write_text_file(&events.with_file_name("messages.jsonl"), "");

    assert_file_check_error_contains(
        &root,
        "home/1000/agent/executor/session/default/events.jsonl",
        &["provider native field"],
    );

    let model_events = fixture_path(
        &root,
        &[
            "shared",
            "project-a",
            "model",
            "openai",
            "gpt-5.6.d",
            "session",
            "default",
            "events.jsonl",
        ],
    );
    write_text_file(
        &model_events,
        "{\"type\":\"done\",\"run\":\"r1\",\"status\":\"ok\"}\n"
    );
    write_text_file(&model_events.with_file_name("messages.jsonl"), "");
    assert!(file_check(
        &root,
        "shared/project-a/model/openai/gpt-5.6.d/session/default/events.jsonl"
    )
    .is_ok());
}

#[test]
fn file_check_rejects_session_streams_without_final_newline() {
    let root = clean_test_dir("ctx-stream-newline-check");
    let session = fixture_path(
        &root,
        &["home", "1000", "agent", "executor", "session", "default"],
    );
    let messages = session.join("messages.jsonl");
    let events = session.join("events.jsonl");

    write_text_file(&events, "");
    write_text_file(&messages, "{\"role\":\"user\",\"content\":\"hello\"}");
    let message_error = file_check(
        &root,
        "home/1000/agent/executor/session/default/messages.jsonl",
    );
    assert_eq!(
        message_error,
        Err(CliError::usage(
            "invalid message stream: missing final newline line 1"
        ))
    );

    write_text_file(&events, "{\"type\":\"done\",\"run\":\"r1\",\"status\":\"ok\"}");
    let event_error = file_check(
        &root,
        "home/1000/agent/executor/session/default/events.jsonl",
    );
    assert_eq!(
        event_error,
        Err(CliError::usage(
            "invalid event stream: missing final newline line 1"
        ))
    );

    write_text_file(
        &messages,
        "{\"role\":\"user\",\"content\":\"hello\"}\n",
    );
    write_text_file(
        &events,
        "{\"type\":\"done\",\"run\":\"r1\",\"status\":\"ok\"}\n",
    );
    assert!(
        file_check(
            &root,
            "home/1000/agent/executor/session/default/messages.jsonl"
        )
        .is_ok()
    );
    assert!(
        file_check(
            &root,
            "home/1000/agent/executor/session/default/events.jsonl"
        )
        .is_ok()
    );
    assert!(!session.join(".store").exists());
}

#[test]
fn file_check_reads_projected_columnar_session_streams() {
    let root = clean_test_dir("ctx-columnar-stream-check");
    let session = fixture_path(
        &root,
        &["home", "1000", "agent", "executor", "session", "default"],
    );
    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    assert!(
        columnar::append(
            &session,
            columnar::Stream::Messages,
            &[r#"{"role":"user"}"#],
        )
        .is_ok()
    );
    assert!(
        columnar::append(
            &session,
            columnar::Stream::Events,
            &[r#"{"type":"native_thread"}"#],
        )
        .is_ok()
    );
    assert_eq!(
        fs::read_to_string(session.join("messages.jsonl")).unwrap_or_default(),
        ""
    );
    assert_eq!(
        fs::read_to_string(session.join("events.jsonl")).unwrap_or_default(),
        ""
    );

    assert_file_check_error_contains(
        &root,
        "home/1000/agent/executor/session/default/messages.jsonl",
        &["missing content line 1"],
    );
    assert_file_check_error_contains(
        &root,
        "home/1000/agent/executor/session/default/events.jsonl",
        &["unknown type line 1 native_thread"],
    );
}
