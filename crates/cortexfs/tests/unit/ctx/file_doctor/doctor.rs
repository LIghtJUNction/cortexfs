#[test]
fn file_check_reports_agent_schedule_permission_denial() {
    let root = clean_test_dir("ctx-agent-schedule-plan-permission-check");
    let control = fixture_path(&root, &["agent", "planner.d"]);
    write_text_file(&control.join("label"), "user_u:agent_r:planner_t:s0\n");
    write_text_file(&control.join("policy"), "allow planner_t agent:reviewer create\n");
    let plan = fixture_path(
        &root,
        &[
            "home",
            "1000",
            "agent",
            "planner",
            "session",
            "default",
            "context",
            "plan.json",
        ],
    );
    write_text_file(
        &plan,
        r#"{"version":1,"mode":"dag-react","nodes":[{"id":"review","kind":"react","agent":"reviewer","child":"review-child","handoff":"Review.","max_steps":3,"requires":[{"class":"tool","name":"fs.read","permission":"execute"}]}]}"#,
    );

    assert_file_check_error_contains(
        &root,
        "home/1000/agent/planner/session/default/context/plan.json",
        &["invalid agent schedule", "permission not granted", "tool:fs.read"],
    );
}

#[test]
fn file_check_reports_agent_schedule_shape_errors() {
    let root = clean_test_dir("ctx-agent-schedule-plan-shape-check");
    let control = fixture_path(&root, &["agent", "planner.d"]);
    write_text_file(&control.join("policy"), "allow planner tool:fs.read execute\n");
    let plan = fixture_path(
        &root,
        &[
            "shared",
            "project-a",
            "agent",
            "planner",
            "session",
            "default",
            "context",
            "plan.json",
        ],
    );
    write_text_file(
        &plan,
        r#"{"version":1,"mode":"dag-react","nodes":[{"id":"a","kind":"react","agent":"reviewer","max_steps":65}]}"#,
    );

    assert_file_check_error_contains(
        &root,
        "shared/project-a/agent/planner/session/default/context/plan.json",
        &["invalid agent schedule", "invalid react bound node a"],
    );
}

#[test]
fn file_check_rejects_agent_schedule_with_invalid_parent_label() {
    let root = clean_test_dir("ctx-agent-schedule-plan-label-check");
    let control = fixture_path(&root, &["agent", "planner.d"]);
    write_text_file(&control.join("label"), "user_u:agent_r:planner/t:s0\n");
    write_text_file(&control.join("policy"), "allow planner_t tool:fs.read execute\n");
    let plan = fixture_path(
        &root,
        &[
            "home",
            "1000",
            "agent",
            "planner",
            "session",
            "default",
            "context",
            "plan.json",
        ],
    );
    write_text_file(
        &plan,
        r#"{"version":1,"mode":"dag-react","nodes":[{"id":"a","kind":"dag","agent":"planner"}]}"#,
    );

    assert_file_check_error_contains(
        &root,
        "home/1000/agent/planner/session/default/context/plan.json",
        &["invalid parent agent label agent/planner.d/label"],
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
fn doctor_rejects_missing_or_drifted_bootstrap_state() {
    let root = clean_test_dir("ctx-doctor-bootstrap-state");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    assert_eq!(doctor_bootstrap_state(&root), Ok(true));

    assert!(fs::remove_file(root.join("bin/cortexfs.bootstrap.json")).is_ok());
    assert_eq!(doctor_bootstrap_state(&root), Ok(false));
    assert!(doctor(&root).is_err());

    write_text_file(
        &root.join("bin/cortexfs.bootstrap.json"),
        r#"{"schema":2,"tree_version":1,"managed_agents":["architect"],"applied_migrations":[]}"#,
    );
    assert_eq!(doctor_bootstrap_state(&root), Ok(false));
    assert!(doctor(&root).is_err());
}

#[test]
fn doctor_counts_present_retired_agents_as_failure() {
    let root = clean_test_dir("ctx-doctor-retired-agent");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    write_text_file(&root.join("agent/base"), "#!/bin/sh\n");

    assert_eq!(doctor_retired_reference_agents(&root), Ok(false));
    assert!(doctor(&root).is_err());
}

#[test]
fn doctor_reports_reference_tree_layout_breakage() {
    let root = clean_test_dir("ctx-doctor-reference-tree-bad");
    let ensured = ensure_v1_reference_tree(&root);
    assert!(ensured.is_ok());
    assert!(fs::remove_file(root.join("tool").join("tsh.d").join("schema")).is_ok());
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
fn doctor_rejects_symlink_root_entries_without_following() {
    let root = clean_test_dir("ctx-doctor-root-symlink-entry");
    let outside = clean_test_dir("ctx-doctor-root-symlink-entry-outside");
    let ensured = ensure_v1_reference_tree(&root);
    assert!(ensured.is_ok());
    assert!(fs::remove_dir_all(root.join("shared")).is_ok());
    assert!(fs::create_dir_all(outside.join("shared").join("project-a")).is_ok());
    assert!(std::os::unix::fs::symlink(outside.join("shared"), root.join("shared")).is_ok());

    let checked = doctor(&root);

    assert!(matches!(
        checked,
        Err(ref error) if error.code == 69 && error.message.contains("doctor found ABI problems")
    ));
}

#[test]
fn doctor_lines_escape_terminal_controls() {
    let root_line = doctor_root_line("ok", Path::new("/tmp/ctx\u{1b}]52;c;payload\u{7}"));
    let entry_line = doctor_unexpected_entry_line("shared\u{1b}[31m");
    let report_line = doctor_report_line(
        "invalid",
        "shared/project\u{1b}[31m/queue",
        Some("missing directory done\u{7}"),
    );

    assert_eq!(
        root_line,
        "ok root /tmp/ctx\\u{1b}]52;c;payload\\u{7}"
    );
    assert_eq!(entry_line, "unexpected shared\\u{1b}[31m");
    assert_eq!(
        report_line,
        "invalid shared/project\\u{1b}[31m/queue: missing directory done\\u{7}"
    );
    assert!(!root_line.as_bytes().contains(&0x1b));
    assert!(!entry_line.as_bytes().contains(&0x1b));
    assert!(!report_line.as_bytes().contains(&0x1b));
    assert!(!root_line.as_bytes().contains(&0x07));
    assert!(!report_line.as_bytes().contains(&0x07));
}

#[test]
fn formats_shared_queue_layout_issues_for_doctor() {
    assert_eq!(
        format_shared_queue_layout_issues(&[
            PathLayoutIssue::missing("done".to_owned(), LayoutPathRole::Directory),
            PathLayoutIssue::wrong_kind("failed".to_owned(), LayoutPathRole::Directory),
        ]),
        "missing directory done, not directory failed"
    );
}

#[test]
fn formats_object_layout_issues_for_file_check() {
    assert_eq!(
        format_object_layout_issues(&[
            PathLayoutIssue::missing("agent/coder".to_owned(), LayoutPathRole::Executable),
            PathLayoutIssue::invalid_value("model/openai/gpt-4o.d/session".to_owned(), "native_thread".to_owned()),
        ]),
        "missing executable agent/coder, invalid value model/openai/gpt-4o.d/session=native_thread"
    );
}

#[test]
fn file_writes_end_in_newline() {
    let root = clean_test_dir("file-writes-newline");
    assert!(fs::create_dir_all(root.join("shared")).is_ok());

    assert!(file_set(&root, "shared/note", "cwd=/work").is_ok());
    assert_eq!(
        fs::read_to_string(root.join("shared/note")).ok().as_deref(),
        Some("cwd=/work\n")
    );

    assert!(file_append(&root, "shared/note", "status=idle").is_ok());
    assert_eq!(
        fs::read_to_string(root.join("shared/note")).ok().as_deref(),
        Some("cwd=/work\nstatus=idle\n")
    );
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
