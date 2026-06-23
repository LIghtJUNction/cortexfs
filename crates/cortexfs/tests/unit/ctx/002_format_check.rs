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
    let root = clean_test_dir("ctx-session-control-check");
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
    assert_file_check_error_contains(
        &root,
        "home/1000/agent/coder/session/default/cwd",
        &["invalid value"],
    );
}

#[test]
fn file_check_validates_agent_control_files() {
    let root = clean_test_dir("ctx-agent-control-check");
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
    assert_file_check_error_contains(&root, "agent/coder.d/life", &["invalid value"]);
    assert!(file_check(&root, "agent/coder.d/parent").is_ok());
}

#[test]
fn file_check_validates_session_index_files() {
    let root = clean_test_dir("ctx-session-index-check");
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

    assert_file_check_error_contains(
        &root,
        "shared/im-qq-dev/agent/bot/session/index/list",
        &["invalid session name"],
    );
    assert!(file_check(&root, "shared/im-qq-dev/agent/bot/session/index/current").is_ok());
    assert!(file_check(
        &root,
        "shared/im-qq-dev/agent/bot/session/index/by-cwd/hash-1"
    )
    .is_ok());
}

#[test]
fn file_check_rejects_by_cwd_symlink_index_entries() {
    let root = clean_test_dir("ctx-session-index-symlink");
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

    assert_file_check_error_contains(
        &root,
        "home/1000/agent/coder/session/index/by-cwd/hash-1",
        &["by-cwd entry is a symlink"],
    );
}

#[test]
fn file_check_validates_model_capability_files() {
    let root = clean_test_dir("ctx-model-cap-check");
    let cap = root.join("model").join("openai").join("gpt-4o.d").join("cap");
    write_text_file(&cap, "chat\nopenai_responses\n");

    assert_file_check_error_contains(
        &root,
        "model/openai/gpt-4o.d/cap",
        &["provider private capability"],
    );

    write_text_file(&cap, "chat\nstream\n");
    let checked = file_check(&root, "model/openai/gpt-4o.d/cap");
    assert!(checked.is_ok());
}

#[test]
fn file_check_validates_model_driver_route_files() {
    let root = clean_test_dir("ctx-model-driver-check");
    let driver = root
        .join("model")
        .join("openai")
        .join("gpt-4o.d")
        .join("driver");
    write_text_file(&driver, "agent=/bin/sh\n");

    assert_file_check_error_contains(
        &root,
        "model/openai/gpt-4o.d/driver",
        &["invalid driver name"],
    );

    write_text_file(
        &driver,
        "default=openai-chat\nexec=openai-chat\nagent=openai-responses,openai-chat\n"
    );
    let checked = file_check(&root, "model/openai/gpt-4o.d/driver");
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
    let schema = root
        .join("shared")
        .join("project-a")
        .join("tool")
        .join("project.test.d")
        .join("schema");
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
    let events = root
        .join("home")
        .join("1000")
        .join("agent")
        .join("coder")
        .join("session")
        .join("default")
        .join("events.jsonl");
    write_text_file(
        &events,
        "{\"type\":\"start\",\"run\":\"r1\",\"response_id\":\"resp_1\"}\n"
    );

    assert_file_check_error_contains(
        &root,
        "home/1000/agent/coder/session/default/events.jsonl",
        &["provider native field"],
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
    write_text_file(
        &model_events,
        "{\"type\":\"done\",\"run\":\"r1\",\"status\":\"ok\"}\n"
    );
    assert!(file_check(
        &root,
        "shared/project-a/model/openai/gpt-4o.d/session/default/events.jsonl"
    )
    .is_ok());
}
