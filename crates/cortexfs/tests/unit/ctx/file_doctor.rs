#[test]
fn file_check_validates_message_stream_files() {
    let root = clean_test_dir("ctx-messages-check");
    let messages = fixture_path(
        &root,
        &[
            "home",
            "1000",
            "agent",
            "coder",
            "session",
            "default",
            "messages.jsonl",
        ],
    );
    write_text_file(
        &messages,
        "{\"role\":\"assistant\",\"response_id\":\"resp_1\",\"content\":\"hello\"}\n"
    );

    assert_file_check_error_contains(
        &root,
        "home/1000/agent/coder/session/default/messages.jsonl",
        &["provider native field"],
    );

    write_text_file(
        &messages,
        "{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}\n"
    );
    assert!(file_check(
        &root,
        "home/1000/agent/coder/session/default/messages.jsonl"
    )
    .is_ok());
}

#[test]
fn file_check_validates_context_jsonl_files() {
    let root = clean_test_dir("ctx-context-jsonl-check");
    let context = fixture_path(
        &root,
        &[
            "shared",
            "project-a",
            "agent",
            "coder",
            "session",
            "default",
            "context",
        ],
    );
    assert!(fs::create_dir_all(context.join("swap")).is_ok());
    write_text_file(
        &context.join("facts.jsonl"),
        "{\"id\":\"f1\",\"text\":\"root is frozen\",\"source\":\"messages:1-2\"}\n"
    );
    write_text_file(
        &context.join("swap").join("index.jsonl"),
        "{\"id\":\"sha256-abc\",\"kind\":\"message_range\",\"source\":\"provider_thread\",\"summary\":\"bad\",\"tokens\":\"10\"}\n",
    );

    assert!(file_check(
        &root,
        "shared/project-a/agent/coder/session/default/context/facts.jsonl"
    )
    .is_ok());
    assert_file_check_error_contains(
        &root,
        "shared/project-a/agent/coder/session/default/context/swap/index.jsonl",
        &["invalid context jsonl"],
    );
}

#[test]
fn file_check_validates_shared_and_model_session_layouts() {
    let root = clean_test_dir("ctx-shared-model-session-check");
    let shared_agent = fixture_path(
        &root,
        &["shared", "im-qq-dev", "agent", "bot", "session", "group-456"],
    );
    let model_session = fixture_path(
        &root,
        &[
            "home", "1000", "model", "openai", "gpt-4o.d", "session", "default",
        ],
    );
    create_complete_session_layout(&shared_agent);
    create_complete_session_layout(&model_session);

    assert!(file_check(&root, "shared/im-qq-dev/agent/bot/session/group-456").is_ok());
    assert!(file_check(
        &root,
        "home/1000/model/openai/gpt-4o.d/session/default"
    )
    .is_ok());

    assert!(fs::remove_file(model_session.join("messages.jsonl")).is_ok());
    assert_file_check_error_contains(
        &root,
        "home/1000/model/openai/gpt-4o.d/session/default",
        &["missing file messages.jsonl"],
    );
}

#[test]
fn doctor_validates_reference_tree_objects_sessions_and_queue() {
    let root = clean_test_dir("ctx-doctor-reference-tree");
    let ensured = ensure_v1_reference_tree(&root);
    assert!(ensured.is_ok());

    assert!(doctor(&root).is_ok());
}

#[test]
fn doctor_reports_reference_tree_layout_breakage() {
    let root = clean_test_dir("ctx-doctor-reference-tree-bad");
    let ensured = ensure_v1_reference_tree(&root);
    assert!(ensured.is_ok());
    assert!(fs::remove_file(root.join("tool").join("fs.read.d").join("schema")).is_ok());
    assert!(
        fs::remove_dir_all(fixture_path(
            &root,
            &[
                "home", "1000", "agent", "coder", "session", "index", "by-cwd",
            ],
        ))
        .is_ok()
    );
    let checked = doctor(&root);
    assert!(matches!(
        checked,
        Err(ref error) if error.code == 69 && error.message.contains("doctor found ABI problems")
    ));
}

#[test]
fn formats_shared_queue_layout_issues_for_doctor() {
    assert_eq!(
        format_shared_queue_layout_issues(&[
            SharedQueueLayoutIssue::MissingDirectory("done".to_owned()),
            SharedQueueLayoutIssue::NotDirectory("failed".to_owned()),
        ]),
        "missing directory done, not directory failed"
    );
}

#[test]
fn formats_object_layout_issues_for_file_check() {
    assert_eq!(
        format_object_layout_issues(&[
            ObjectLayoutIssue::MissingExecutable("agent/coder".to_owned()),
            ObjectLayoutIssue::InvalidControlValue {
                path: "model/openai/gpt-4o.d/session".to_owned(),
                value: "native_thread".to_owned(),
            },
        ]),
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
