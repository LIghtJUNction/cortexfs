#[test]
fn agent_wait_reaps_active_child_when_backing_worker_is_dead() {
    let root = clean_test_dir("ctx-agent-wait-reaps-dead-worker");
    let ensured = ensure_v1_reference_tree(&root);
    assert!(ensured.is_ok());
    let session = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );
    create_complete_session_layout(&session);
    let child = session.join("context").join("child").join("work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "active\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(&child.join("result.md"), "");
    write_text_file(&child.join("refs.jsonl"), "");
    write_text_file(
        &root.join("agent/worker.d/parent"),
        "agent:coder session:default run:r1\n",
    );
    write_text_file(&root.join("agent/worker.d/status"), "dead\n");
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");
    write_text_file(&root.join("agent/worker.d/pid"), "\n");

    let rows = agent_child_rows(&root, "coder", Some("default"));
    assert!(matches!(
        rows,
        Ok(ref rows) if rows.contains(&AgentChildRow {
            child: "work-123".to_owned(),
            status: "cancelled".to_owned(),
            agent: "worker".to_owned(),
            session: "default".to_owned(),
            parent_session: Some("default".to_owned()),
            model: "api.lmm.best/gpt-5.3-codex-spark".to_owned(),
            life: "temp".to_owned(),
            agent_status: "dead".to_owned(),
            pid: None,
        })
    ));
    assert_eq!(
        agent_wait(&root, "coder", Some("default"), "work-123"),
        Ok(ExitCode::from(130))
    );
    assert!(matches!(
        fs::read_to_string(child.join("status")).as_deref(),
        Ok("cancelled\n")
    ));
    assert!(matches!(
        fs::read_to_string(child.join("result.md")).as_deref(),
        Ok("Child agent `worker` session `default` is dead.\n")
    ));
}

#[test]
fn agent_child_rows_default_missing_worker_model_to_spark() {
    let root = clean_test_dir("ctx-child-row-missing-worker-model");
    let ensured = ensure_v1_reference_tree(&root);
    assert!(ensured.is_ok());
    assert!(fs::remove_file(root.join("agent/worker.d/model")).is_ok());
    write_text_file(&root.join("agent/worker.d/status"), "idle\n");
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");
    let session = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );
    create_complete_session_layout(&session);
    let child = session.join("context").join("child").join("work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "done\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(&child.join("result.md"), "Done.\n");
    write_text_file(&child.join("refs.jsonl"), "");

    let rows = agent_child_rows(&root, "coder", Some("default"));
    assert!(matches!(
        rows,
        Ok(ref rows) if rows.contains(&AgentChildRow {
            child: "work-123".to_owned(),
            status: "done".to_owned(),
            agent: "worker".to_owned(),
            session: "default".to_owned(),
            parent_session: None,
            model: "api.lmm.best/gpt-5.3-codex-spark".to_owned(),
            life: "temp".to_owned(),
            agent_status: "idle".to_owned(),
            pid: None,
        })
    ));
}

#[test]
fn agent_wait_reaps_active_child_when_parent_session_is_omitted() {
    let root = clean_test_dir("ctx-wait-reaps-dead-no-session");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let session = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );
    create_complete_session_layout(&session);
    let child = session.join("context").join("child").join("work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "active\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(&child.join("result.md"), "");
    write_text_file(&child.join("refs.jsonl"), "");
    write_text_file(&root.join("agent/worker.d/parent"), "agent:coder run:r1\n");
    write_text_file(&root.join("agent/worker.d/status"), "dead\n");
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");
    write_text_file(&root.join("agent/worker.d/pid"), "\n");

    assert_eq!(
        agent_wait(&root, "coder", Some("default"), "work-123"),
        Ok(ExitCode::from(130))
    );
    assert!(matches!(
        fs::read_to_string(child.join("status")).as_deref(),
        Ok("cancelled\n")
    ));
    assert!(matches!(
        fs::read_to_string(child.join("result.md")).as_deref(),
        Ok("Child agent `worker` session `default` is dead.\n")
    ));
}

#[test]
fn agent_wait_reaps_active_child_when_backing_worker_pid_is_stale() {
    let root = clean_test_dir("ctx-agent-wait-reaps-stale-worker");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let session = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );
    create_complete_session_layout(&session);
    let child = session.join("context").join("child").join("work-stale");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "active\n");
    write_text_file(&child.join("handoff.md"), "Task: implement.\n");
    write_text_file(&child.join("result.md"), "");
    write_text_file(&child.join("refs.jsonl"), "");
    write_text_file(
        &root.join("agent/worker.d/parent"),
        "agent:coder session:default run:r1\n",
    );
    write_text_file(&root.join("agent/worker.d/status"), "busy\n");
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");
    write_text_file(&root.join("agent/worker.d/pid"), "999999999\n");

    let rows = agent_child_rows(&root, "coder", Some("default"));
    assert!(matches!(
        rows,
        Ok(ref rows) if rows.contains(&AgentChildRow {
            child: "work-stale".to_owned(),
            status: "cancelled".to_owned(),
            agent: "worker".to_owned(),
            session: "default".to_owned(),
            parent_session: Some("default".to_owned()),
            model: "api.lmm.best/gpt-5.3-codex-spark".to_owned(),
            life: "temp".to_owned(),
            agent_status: "dead".to_owned(),
            pid: None,
        })
    ));
    assert_eq!(
        agent_wait(&root, "coder", Some("default"), "work-stale"),
        Ok(ExitCode::from(130))
    );
    assert!(matches!(
        fs::read_to_string(child.join("result.md")).as_deref(),
        Ok("Child agent `worker` session `default` is dead.\n")
    ));
}

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
