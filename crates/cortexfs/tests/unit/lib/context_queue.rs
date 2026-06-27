#[test]
fn context_pack_rebuild_respects_budget_and_validates_inputs() {
    let root = clean_test_dir("context-pack-rebuild-budget");
    let session = root.join("default");
    let context = session.join("context");

    create_complete_session_layout(&session);
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"one two three four five six\"}\n",
    );
    write_text_file(&context.join("budget"), "2\n");
    write_text_file(&context.join("summary.md"), "one two\n");
    write_text_file(&context.join("facts.jsonl"), "");
    write_text_file(&context.join("decisions.jsonl"), "");
    write_text_file(&context.join("todo.md"), "");
    write_text_file(&context.join("refs.jsonl"), "");
    write_text_file(&context.join("child").join("rev-1").join("result.md"), "");
    write_text_file(&context.join("child").join("rev-1").join("refs.jsonl"), "");

    let built = rebuild_context_pack(&session, Some("coder"), 5);
    let built = ok!(built);
    assert_eq!(built.items().len(), 1);
    assert_eq!(
        built
            .items()
            .first()
            .map(super::ContextPackBuiltItem::source),
        Some("context/summary.md")
    );
    assert!(!built.pack_json().contains("messages.jsonl"));

    write_text_file(&context.join("budget"), " 2\n");
    assert_eq!(
        rebuild_context_pack(&session, Some("coder"), 5),
        Err(ContextPackBuildError::InvalidBudget)
    );
    write_text_file(&context.join("budget"), "0\n");
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"native_thread\"}\n",
    );
    assert_eq!(
        rebuild_context_pack(&session, Some("coder"), 5),
        Err(ContextPackBuildError::InvalidMessages)
    );
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"ok\"}\n",
    );
    assert_eq!(
        rebuild_context_pack(&session, Some("bad/agent"), 5),
        Err(ContextPackBuildError::InvalidAgentName)
    );
    assert!(fs::create_dir_all(context.join("child").join(".bad")).is_ok());
    assert_eq!(
        rebuild_context_pack(&session, Some("coder"), 5),
        Err(ContextPackBuildError::InvalidChildName)
    );
}

#[test]
fn context_pack_rebuild_rejects_symlink_session_files() {
    let root = clean_test_dir("context-pack-rebuild-symlink-session");
    let session = root.join("default");
    let context = session.join("context");
    let outside = clean_test_dir("context-pack-rebuild-symlink-session-outside");

    create_complete_session_layout(&session);
    write_text_file(&outside.join("messages.jsonl"), "{\"role\":\"user\",\"content\":\"outside\"}\n");
    assert!(fs::remove_file(session.join("messages.jsonl")).is_ok());
    assert!(symlink(outside.join("messages.jsonl"), session.join("messages.jsonl")).is_ok());
    write_text_file(&context.join("budget"), "0\n");

    assert_eq!(
        rebuild_context_pack(&session, Some("coder"), 5),
        Err(ContextPackBuildError::MissingSession)
    );
}

#[test]
fn context_pack_rebuild_ignores_symlink_pinned_files() {
    let root = clean_test_dir("context-pack-rebuild-symlink-pinned");
    let session = root.join("default");
    let context = session.join("context");
    let outside = clean_test_dir("context-pack-rebuild-symlink-pinned-outside");

    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "{\"role\":\"user\",\"content\":\"ok\"}\n");
    write_text_file(&context.join("budget"), "0\n");
    write_text_file(&context.join("summary.md"), "");
    write_text_file(&context.join("facts.jsonl"), "");
    write_text_file(&context.join("decisions.jsonl"), "");
    write_text_file(&context.join("todo.md"), "");
    write_text_file(&context.join("refs.jsonl"), "");
    write_text_file(&context.join("child").join("rev-1").join("result.md"), "");
    write_text_file(&context.join("child").join("rev-1").join("refs.jsonl"), "");
    write_text_file(&outside.join("system.md"), "outside pinned\n");
    assert!(symlink(
        outside.join("system.md"),
        context.join("pinned").join("system.md")
    )
    .is_ok());

    let built = rebuild_context_pack(&session, Some("coder"), 5);
    let built = ok!(built);
    assert!(!built.pack_md().contains("outside pinned"));
    assert!(!built
        .items()
        .iter()
        .any(|item| item.source() == "context/pinned/system.md"));
}

#[test]
fn context_pack_rebuild_ignores_control_character_pinned_files() {
    let root = clean_test_dir("context-pack-rebuild-control-pinned");
    let session = root.join("default");
    let context = session.join("context");

    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "{\"role\":\"user\",\"content\":\"ok\"}\n");
    write_text_file(&context.join("budget"), "0\n");
    write_text_file(&context.join("summary.md"), "");
    write_text_file(&context.join("facts.jsonl"), "");
    write_text_file(&context.join("decisions.jsonl"), "");
    write_text_file(&context.join("todo.md"), "");
    write_text_file(&context.join("refs.jsonl"), "");
    write_text_file(&context.join("child").join("rev-1").join("result.md"), "");
    write_text_file(&context.join("child").join("rev-1").join("refs.jsonl"), "");
    write_text_file(&context.join("pinned").join("bad\u{1b}.md"), "hidden pinned\n");

    let built = rebuild_context_pack(&session, Some("coder"), 5);
    let built = ok!(built);
    assert!(!built.pack_md().contains("hidden pinned"));
}

#[test]
fn context_pack_rebuild_refuses_symlink_pinned_directory() {
    let root = clean_test_dir("context-pack-rebuild-symlink-pinned-dir");
    let session = root.join("default");
    let context = session.join("context");
    let outside = clean_test_dir("context-pack-rebuild-symlink-pinned-dir-outside");

    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "{\"role\":\"user\",\"content\":\"ok\"}\n");
    write_text_file(&context.join("budget"), "0\n");
    write_text_file(&outside.join("system.md"), "outside pinned\n");
    assert!(fs::remove_dir(context.join("pinned")).is_ok());
    assert!(symlink(&outside, context.join("pinned")).is_ok());

    assert_eq!(
        rebuild_context_pack(&session, Some("coder"), 5),
        Err(ContextPackBuildError::MissingSession)
    );
}

#[test]
fn context_pack_rebuild_refuses_symlink_child_directory() {
    let root = clean_test_dir("context-pack-rebuild-symlink-child-dir");
    let session = root.join("default");
    let context = session.join("context");
    let outside = clean_test_dir("context-pack-rebuild-symlink-child-dir-outside");

    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "{\"role\":\"user\",\"content\":\"ok\"}\n");
    write_text_file(&context.join("budget"), "0\n");
    write_text_file(&outside.join("rev-1").join("result.md"), "outside result\n");
    assert!(fs::remove_dir_all(context.join("child")).is_ok());
    assert!(symlink(&outside, context.join("child")).is_ok());

    assert_eq!(
        rebuild_context_pack(&session, Some("coder"), 5),
        Err(ContextPackBuildError::MissingSession)
    );
}

#[test]
fn context_pack_rebuild_rejects_oversized_messages_file() {
    let root = clean_test_dir("context-pack-rebuild-oversized-messages");
    let session = root.join("default");
    let context = session.join("context");

    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), &"x".repeat((1024 * 1024) + 1));
    write_text_file(&context.join("budget"), "0\n");

    assert_eq!(
        rebuild_context_pack(&session, Some("coder"), 5),
        Err(ContextPackBuildError::CannotRead)
    );
}

#[test]
fn context_pack_rebuild_rejects_oversized_context_sources() {
    let root = clean_test_dir("context-pack-rebuild-oversized-source");
    let session = root.join("default");
    let context = session.join("context");

    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "{\"role\":\"user\",\"content\":\"ok\"}\n");
    write_text_file(&context.join("budget"), "0\n");
    write_text_file(&context.join("summary.md"), &"x".repeat((1024 * 1024) + 1));
    write_text_file(&context.join("facts.jsonl"), "");
    write_text_file(&context.join("decisions.jsonl"), "");
    write_text_file(&context.join("todo.md"), "");
    write_text_file(&context.join("refs.jsonl"), "");
    write_text_file(&context.join("child").join("rev-1").join("result.md"), "");
    write_text_file(&context.join("child").join("rev-1").join("refs.jsonl"), "");

    assert_eq!(
        rebuild_context_pack(&session, Some("coder"), 5),
        Err(ContextPackBuildError::CannotRead)
    );
}

#[test]
fn context_pack_rejects_invalid_json_shape() {
    assert_eq!(
        inspect_context_pack_json("{").issues(),
        &[ContextPackIssue::InvalidJson]
    );
    assert_eq!(
        inspect_context_pack_json(r#"{"items": []} trailing"#).issues(),
        &[ContextPackIssue::InvalidJson]
    );
    assert_eq!(
        inspect_context_pack_json(r#"{"items": {"source": "messages.jsonl"}}"#).issues(),
        &[ContextPackIssue::ItemsNotArray]
    );
    assert_eq!(
        inspect_context_pack_json(r#"{"items":[],"items":[]}"#).issues(),
        &[ContextPackIssue::ItemsNotArray]
    );
    assert_eq!(
        inspect_context_pack_json(r#"{"items": ["messages.jsonl"]}"#).issues(),
        &[ContextPackIssue::ItemNotObject(0)]
    );
}

#[test]
fn message_stream_accepts_canonical_role_content_frames() {
    let report = inspect_message_stream_jsonl(
        r#"{"role":"system","content":"You are concise."}
{"role":"user","content":[{"type":"text","text":"hello"}]}
{"role":"assistant","content":[{"type":"text","text":"hi"}]}
{"role":"tool","content":[{"type":"tool_result","tool_call_id":"call-1","content":"ok"}]}
"#,
    );
    assert!(report.is_ok());
    assert!(report.issues().is_empty());
}

#[test]
fn message_stream_rejects_native_state_and_bad_shape() {
    let report = inspect_message_stream_jsonl(
        r#"not-json
[]
{"content":"missing role"}
{"role":"developer","content":"private role"}
{"role":"assistant","response_id":"resp-1","content":"hi"}
{"role":"assistant","content":[{"type":"provider_blob","text":"x"}]}
{"role":"assistant"}
"#,
    );
    assert_eq!(
        report.issues(),
        [
            MessageStreamIssue::InvalidJson(1),
            MessageStreamIssue::MessageNotObject(2),
            MessageStreamIssue::MissingRole(3),
            MessageStreamIssue::InvalidRole {
                line: 4,
                role: "developer".to_owned()
            },
            MessageStreamIssue::ProviderNativeField {
                line: 5,
                field: "response_id".to_owned()
            },
            MessageStreamIssue::InvalidContent(6),
            MessageStreamIssue::MissingContent(7)
        ]
    );
}

#[test]
fn context_jsonl_accepts_spec_record_shapes() {
    assert!(inspect_context_jsonl(
        ContextJsonlKind::Facts,
        r#"{"id":"f1","text":"CortexFS root is small.","source":"messages:12-18"}
"#
    )
    .is_ok());
    assert!(inspect_context_jsonl(
        ContextJsonlKind::Decisions,
        r#"{"id":"d1","decision":"Child agents are owned.","source":"user:latest"}
"#
    )
    .is_ok());
    assert!(inspect_context_jsonl(
        ContextJsonlKind::Refs,
        r#"{"id":"r1","path":"/work/DESIGN.md","kind":"file","summary":"design"}
{"id":"r2","path":"context/swap/chunk/sha256-abc","kind":"swap","summary":"old design"}
"#
    )
    .is_ok());
    assert!(
        inspect_context_jsonl(
            ContextJsonlKind::SwapIndex,
            r#"{"id":"sha256-abc","kind":"message_range","source":"messages.jsonl","summary":"initial design","tokens":18000}
{"id":"sha256-def","kind":"tool_output","source":"events.jsonl","summary":"test output","tokens":45000}
"#
        )
        .is_ok()
    );
    assert!(
        inspect_context_jsonl(
            ContextJsonlKind::DedupIndex,
            r#"{"hash":"sha256-abc","refs":["messages:1-40","swap:old-design"],"bytes":12000,"tokens":3000}
"#
        )
        .is_ok()
    );
}

#[test]
fn context_jsonl_rejects_invalid_records() {
    let facts = inspect_context_jsonl(
        ContextJsonlKind::Facts,
        "not-json\n[]\n{\"id\":\"fact-1\",\"text\":\"ok\",\"source\":\"messages:1\"} trailing\n{\"id\":\"bad/id\",\"text\":\"ok\"}\n",
    );
    assert_eq!(
        facts.issues(),
        [
            ContextJsonlIssue::InvalidJson(1),
            ContextJsonlIssue::RecordNotObject(2),
            ContextJsonlIssue::InvalidJson(3),
            ContextJsonlIssue::InvalidField {
                line: 4,
                field: "id".to_owned(),
                value: "bad/id".to_owned()
            },
            ContextJsonlIssue::MissingStringField {
                line: 4,
                field: "source".to_owned()
            }
        ]
    );

    let duplicate = inspect_context_jsonl(
        ContextJsonlKind::Facts,
        "{\"id\":\"fact-1\",\"id\":\"fact-2\",\"text\":\"ok\",\"source\":\"messages:1\"}\n",
    );
    assert_eq!(duplicate.issues(), [ContextJsonlIssue::InvalidJson(1)]);

    let refs = inspect_context_jsonl(
        ContextJsonlKind::Refs,
        r#"{"id":"r1","path":"../secret","kind":"provider_thread","summary":"bad"}
"#,
    );
    assert_eq!(
        refs.issues(),
        [
            ContextJsonlIssue::InvalidField {
                line: 1,
                field: "path".to_owned(),
                value: "../secret".to_owned()
            },
            ContextJsonlIssue::InvalidField {
                line: 1,
                field: "kind".to_owned(),
                value: "provider_thread".to_owned()
            }
        ]
    );

    let control_path = inspect_context_jsonl(
        ContextJsonlKind::Refs,
        "{\"id\":\"r1\",\"path\":\"context/\\u001bhidden.md\",\"kind\":\"file\",\"summary\":\"bad\"}\n",
    );
    assert_eq!(
        control_path.issues(),
        [ContextJsonlIssue::InvalidField {
            line: 1,
            field: "path".to_owned(),
            value: "context/\u{1b}hidden.md".to_owned()
        }]
    );

    let dedup = inspect_context_jsonl(
        ContextJsonlKind::DedupIndex,
        r#"{"hash":"md5-old","refs":[],"bytes":"120","tokens":3000}
"#,
    );
    assert_eq!(
        dedup.issues(),
        [
            ContextJsonlIssue::InvalidField {
                line: 1,
                field: "hash".to_owned(),
                value: "md5-old".to_owned()
            },
            ContextJsonlIssue::MissingStringArrayField {
                line: 1,
                field: "refs".to_owned()
            },
            ContextJsonlIssue::MissingNumberField {
                line: 1,
                field: "bytes".to_owned()
            }
        ]
    );
}

#[test]
fn event_stream_accepts_canonical_model_jsonl() {
    let report = inspect_event_stream_jsonl(
        r#"{"type":"start","run":"r1","model":"debug/echo"}
{"type":"delta","run":"r1","text":"hello"}
{"type":"message","run":"r1","role":"assistant","content":[{"type":"text","text":"hello"}]}
{"type":"tool_call","run":"r1","id":"call-1","name":"fs.read","arguments":{"path":"README.md"}}
{"type":"usage","run":"r1","input_tokens":10,"output_tokens":1}
{"type":"done","run":"r1","status":"ok"}
"#,
    );
    assert!(report.is_ok());
    assert!(report.issues().is_empty());
}

#[test]
fn event_stream_accepts_stable_error_frames() {
    let report = inspect_event_stream_jsonl(
        r#"{"type":"error","run":"r1","code":"EACCES","message":"permission denied"}
{"type":"done","run":"r1","status":"error"}
"#,
    );
    assert!(report.is_ok());
}

#[test]
fn event_stream_accepts_child_lifecycle_frames() {
    let report = inspect_event_stream_jsonl(
        r#"{"type":"agent.child.cancel","parent":"coder","child":"rev-123","reason":"parent_dead"}
{"type":"agent.stop","agent":"rev-123","status":"cancelled"}
"#,
    );
    assert!(report.is_ok());
    assert!(report.issues().is_empty());
}

#[test]
fn event_stream_rejects_provider_native_state_and_unknown_events() {
    let report = inspect_event_stream_jsonl(
        r#"{"type":"start","run":"r1","model":"debug/echo","response_id":"resp_123"}
{"type":"native_thread","run":"r1","thread_id":"thread_123"}
{"type":"message","run":"r1","content":[{"type":"text","text":"x","provider_response_id":"abc"}]}
"#,
    );
    assert_eq!(
        report.issues(),
        [
            EventStreamIssue::ProviderNativeField {
                line: 1,
                field: "response_id".to_owned()
            },
            EventStreamIssue::ProviderNativeField {
                line: 2,
                field: "thread_id".to_owned()
            },
            EventStreamIssue::UnknownType {
                line: 2,
                event_type: "native_thread".to_owned()
            },
            EventStreamIssue::ProviderNativeField {
                line: 3,
                field: "provider_response_id".to_owned()
            }
        ]
    );
}

#[test]
fn event_stream_rejects_invalid_shape_and_specialized_frames() {
    let report = inspect_event_stream_jsonl(
        r#"not-json
[]
{"run":"r1"}
{"type":"delta","text":"missing run"}
{"type":"error","run":"r1","code":"PROVIDER_DENIED"}
{"type":"done","run":"r1","status":"maybe"}
{"type":"usage","run":"r1","input_tokens":"10","output_tokens":1}
{"type":"tool_call","run":"r1","id":"bad/id","name":"fs.read"}
{"type":"agent.child.cancel","parent":"bad/parent","child":"rev-1","reason":"manual"}
{"type":"agent.stop","agent":"rev-1","status":"dead"}
"#,
    );
    assert_eq!(
        report.issues(),
        [
            EventStreamIssue::InvalidJson(1),
            EventStreamIssue::EventNotObject(2),
            EventStreamIssue::MissingType(3),
            EventStreamIssue::MissingRun(4),
            EventStreamIssue::InvalidErrorCode(5),
            EventStreamIssue::InvalidDoneStatus(6),
            EventStreamIssue::InvalidUsage(7),
            EventStreamIssue::InvalidToolCall(8),
            EventStreamIssue::InvalidAgentLifecycle(9),
            EventStreamIssue::InvalidAgentLifecycle(10)
        ]
    );
}

#[test]
fn shared_queue_layout_inspector_checks_recommended_dirs() {
    let root = clean_test_dir("shared-queue-layout");
    for dir in SHARED_QUEUE_REQUIRED_DIRS {
        assert!(fs::create_dir_all(root.join(dir)).is_ok());
    }
    let report = inspect_shared_queue_layout(&root);
    assert!(report.is_ok());

    assert!(fs::remove_dir_all(root.join("failed")).is_ok());
    assert!(fs::remove_dir_all(root.join("done")).is_ok());
    assert!(fs::write(root.join("done"), "not a dir\n").is_ok());
    let report = inspect_shared_queue_layout(&root);
    assert!(!report.is_ok());
    assert!(report
        .issues()
        .contains(&SharedQueueLayoutIssue::MissingDirectory(
            "failed".to_owned()
        )));
    assert!(report
        .issues()
        .contains(&SharedQueueLayoutIssue::NotDirectory("done".to_owned())));
}

#[test]
fn shared_queue_layout_rejects_symlink_directories() {
    let root = clean_test_dir("shared-queue-layout-symlink");
    create_shared_queue_layout(&root);
    let outside = clean_test_dir("shared-queue-layout-symlink-outside");
    assert!(fs::remove_dir_all(root.join("pending")).is_ok());
    assert!(symlink(&outside, root.join("pending")).is_ok());

    let report = inspect_shared_queue_layout(&root);
    assert!(report
        .issues()
        .contains(&SharedQueueLayoutIssue::NotDirectory(
            "pending".to_owned()
        )));
    assert_eq!(
        claim_next_shared_queue_job(&root, "worker-a"),
        Err(SharedQueueClaimError::InvalidQueueDirectory)
    );
}

#[test]
fn shared_queue_claim_uses_atomic_claim_directories() {
    let root = clean_test_dir("shared-queue-claim");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-2.req.json"), "two\n");
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    write_text_file(&root.join("pending").join(".ignored"), "bad\n");
    assert!(fs::create_dir_all(root.join("pending").join("not-file")).is_ok());

    let first = claim_next_shared_queue_job(&root, "worker-a");
    let Some(first) = ok!(first) else { return };
    assert_eq!(first.job_name(), "job-1.req.json");
    assert_file_text(first.claimed_path(), "one\n");
    assert_file_text(&first.lease_path().join("worker"), "worker-a\n");
    assert!(!root.join("pending").join("job-1.req.json").exists());

    let second = claim_next_shared_queue_job(&root, "worker-b");
    let Some(second) = ok!(second) else { return };
    assert_eq!(second.job_name(), "job-2.req.json");

    let none = claim_next_shared_queue_job(&root, "worker-c");
    assert_eq!(none, Ok(None));
}

#[test]
fn shared_queue_claim_ignores_non_req_json_and_symlink_jobs() {
    let root = clean_test_dir("shared-queue-claim-ignore");
    create_shared_queue_layout(&root);
    let outside = clean_test_dir("shared-queue-claim-ignore-outside");
    write_text_file(&outside.join("secret.txt"), "secret\n");
    assert!(symlink(
        outside.join("secret.txt"),
        root.join("pending").join("job-0.req.json")
    )
    .is_ok());
    write_text_file(&root.join("pending").join("job-1.req.json.tmp"), "tmp\n");
    write_text_file(&root.join("pending").join("job-2.req.json"), "real\n");

    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    assert_eq!(claimed.job_name(), "job-2.req.json");
    assert!(root.join("pending").join("job-0.req.json").exists());
    assert!(root.join("pending").join("job-1.req.json.tmp").exists());
}

#[test]
fn shared_queue_claim_skips_existing_claim_lock() {
    let root = clean_test_dir("shared-queue-claim-lock");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    write_text_file(&root.join("pending").join("job-2.req.json"), "two\n");
    assert!(fs::create_dir_all(root.join("claimed").join("job-1.req.json")).is_ok());

    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    assert_eq!(claimed.job_name(), "job-2.req.json");
    assert!(root.join("pending").join("job-1.req.json").exists());
}

#[test]
fn shared_queue_claim_rolls_back_when_lease_recording_fails() {
    let root = clean_test_dir("shared-queue-claim-lease-fail");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    assert!(fs::write(root.join("lease").join("job-1.req.json"), "not a dir\n").is_ok());

    assert_eq!(
        claim_next_shared_queue_job(&root, "worker-a"),
        Err(SharedQueueClaimError::CannotRecordLease)
    );
    assert_file_text(&root.join("pending").join("job-1.req.json"), "one\n");
    assert!(!root.join("claimed").join("job-1.req.json").exists());
}

#[test]
fn shared_queue_claim_rejects_symlink_queue_root_without_touching_target() {
    let root = clean_test_dir("shared-queue-claim-symlink-root");
    let outside = clean_test_dir("shared-queue-claim-symlink-root-outside");
    create_shared_queue_layout(&outside);
    write_text_file(&outside.join("pending").join("job-1.req.json"), "one\n");
    assert!(symlink(&outside, &root).is_ok());

    assert_eq!(
        claim_next_shared_queue_job(&root, "worker-a"),
        Err(SharedQueueClaimError::InvalidQueueDirectory)
    );
    assert_file_text(&outside.join("pending").join("job-1.req.json"), "one\n");
    assert!(!outside.join("claimed").join("job-1.req.json").exists());
}

#[test]
fn shared_queue_finish_and_recover_reject_non_request_job_names() {
    let root = clean_test_dir("shared-queue-invalid-job-name");
    create_shared_queue_layout(&root);

    assert_eq!(
        finish_shared_queue_job(&root, "job-1", SharedQueueOutcome::Done, b"ok\n"),
        Err(SharedQueueFinishError::InvalidJobName)
    );
    assert_eq!(
        finish_shared_queue_job(&root, "job-1.req.json.tmp", SharedQueueOutcome::Done, b"ok\n"),
        Err(SharedQueueFinishError::InvalidJobName)
    );
    assert_eq!(
        recover_shared_queue_job(&root, "job-1"),
        Err(SharedQueueRecoverError::InvalidJobName)
    );
    assert_eq!(
        recover_shared_queue_job(&root, "job-1.req.json.tmp"),
        Err(SharedQueueRecoverError::InvalidJobName)
    );
}

#[test]
fn shared_queue_recovery_requeues_claimed_job_with_lease() {
    let root = clean_test_dir("shared-queue-recover");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");

    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    assert!(claimed.claimed_path().is_file());
    assert!(claimed.lease_path().join("worker").is_file());

    let recovered = recover_shared_queue_job(&root, "job-1.req.json");
    assert_eq!(recovered, Ok(root.join("pending").join("job-1.req.json")));
    assert_file_text(&root.join("pending").join("job-1.req.json"), "one\n");
    assert!(!root.join("claimed").join("job-1.req.json").exists());
    assert!(!root.join("lease").join("job-1.req.json").exists());
}

#[test]
fn shared_queue_finish_does_not_write_result_without_claim() {
    let root = clean_test_dir("shared-queue-finish-without-claim");
    create_shared_queue_layout(&root);

    assert_eq!(
        finish_shared_queue_job(&root, "job-1.req.json", SharedQueueOutcome::Done, b"ok\n"),
        Err(SharedQueueFinishError::CannotMoveClaimedJob)
    );
    assert!(!root.join("done").join("job-1.req.json.result").exists());
}

#[test]
fn shared_queue_finish_refuses_to_overwrite_output_entries() {
    let root = clean_test_dir("shared-queue-finish-no-overwrite");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    write_text_file(&root.join("done").join("job-1.req.json.result"), "old\n");

    assert_eq!(
        finish_shared_queue_job(&root, claimed.job_name(), SharedQueueOutcome::Done, b"new\n"),
        Err(SharedQueueFinishError::CannotWriteResult)
    );
    assert_file_text(&root.join("done").join("job-1.req.json.result"), "old\n");
    assert!(claimed.claimed_path().exists());

    assert!(fs::remove_file(root.join("done").join("job-1.req.json.result")).is_ok());
    write_text_file(&root.join("done").join("job-1.req.json"), "old request\n");
    assert_eq!(
        finish_shared_queue_job(&root, claimed.job_name(), SharedQueueOutcome::Done, b"new\n"),
        Err(SharedQueueFinishError::CannotWriteResult)
    );
    assert_file_text(&root.join("done").join("job-1.req.json"), "old request\n");
    assert!(!root.join("done").join("job-1.req.json.result").exists());
}

#[test]
fn shared_queue_finish_refuses_symlink_result_without_writing_target() {
    let root = clean_test_dir("shared-queue-finish-symlink-result");
    let outside = clean_test_dir("shared-queue-finish-symlink-result-outside");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    write_text_file(&outside.join("result"), "outside\n");
    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    assert!(symlink(
        outside.join("result"),
        root.join("done").join("job-1.req.json.result")
    )
    .is_ok());

    assert_eq!(
        finish_shared_queue_job(&root, claimed.job_name(), SharedQueueOutcome::Done, b"new\n"),
        Err(SharedQueueFinishError::CannotWriteResult)
    );
    assert_file_text(&outside.join("result"), "outside\n");
    assert!(claimed.claimed_path().exists());
}

#[test]
fn shared_queue_recovery_refuses_to_overwrite_pending_job() {
    let root = clean_test_dir("shared-queue-recover-no-overwrite");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    write_text_file(&root.join("pending").join("job-1.req.json"), "new\n");

    assert_eq!(
        recover_shared_queue_job(&root, claimed.job_name()),
        Err(SharedQueueRecoverError::CannotRequeue)
    );
    assert_file_text(&root.join("pending").join("job-1.req.json"), "new\n");
    assert!(claimed.claimed_path().exists());
}

#[test]
fn shared_queue_finish_rejects_symlink_output_directory_without_writing_target() {
    let root = clean_test_dir("shared-queue-finish-symlink-dir");
    let outside = clean_test_dir("shared-queue-finish-symlink-dir-outside");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    assert!(fs::remove_dir_all(root.join("done")).is_ok());
    assert!(symlink(&outside, root.join("done")).is_ok());

    assert_eq!(
        finish_shared_queue_job(&root, claimed.job_name(), SharedQueueOutcome::Done, b"ok\n"),
        Err(SharedQueueFinishError::InvalidQueueDirectory)
    );
    assert!(!outside.join("job-1.req.json.result").exists());
}

#[test]
fn shared_queue_finish_rejects_symlink_lease_directory_without_touching_target() {
    let root = clean_test_dir("shared-queue-finish-symlink-lease");
    let outside = clean_test_dir("shared-queue-finish-symlink-lease-outside");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    assert!(fs::remove_dir_all(root.join("lease").join(claimed.job_name())).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    write_text_file(&outside.join("worker"), "worker-a\n");
    assert!(symlink(&outside, root.join("lease").join(claimed.job_name())).is_ok());

    assert_eq!(
        finish_shared_queue_job(&root, claimed.job_name(), SharedQueueOutcome::Done, b"ok\n"),
        Err(SharedQueueFinishError::CannotMoveClaimedJob)
    );
    assert_file_text(&outside.join("worker"), "worker-a\n");
    assert!(!root.join("done").join(claimed.job_name()).exists());
    assert!(!root
        .join("done")
        .join(format!("{}.result", claimed.job_name()))
        .exists());
}

#[test]
fn shared_queue_finish_rejects_symlink_claim_directory_without_touching_target() {
    let root = clean_test_dir("shared-queue-finish-symlink-claim");
    let outside = clean_test_dir("shared-queue-finish-symlink-claim-outside");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    assert!(fs::remove_dir_all(root.join("claimed").join(claimed.job_name())).is_ok());
    write_text_file(&outside.join(claimed.job_name()), "outside\n");
    assert!(symlink(&outside, root.join("claimed").join(claimed.job_name())).is_ok());

    assert_eq!(
        finish_shared_queue_job(&root, claimed.job_name(), SharedQueueOutcome::Done, b"ok\n"),
        Err(SharedQueueFinishError::CannotMoveClaimedJob)
    );
    assert_file_text(&outside.join(claimed.job_name()), "outside\n");
    assert!(!root.join("done").join(claimed.job_name()).exists());
    assert!(!root
        .join("done")
        .join(format!("{}.result", claimed.job_name()))
        .exists());
}

#[test]
fn shared_queue_recovery_requires_existing_claim_and_lease() {
    let root = clean_test_dir("shared-queue-recover-missing");
    create_shared_queue_layout(&root);
    assert_eq!(
        recover_shared_queue_job(&root, "job-1.req.json"),
        Err(SharedQueueRecoverError::MissingClaim)
    );

    let claim_dir = root.join("claimed").join("job-1.req.json");
    assert!(fs::create_dir_all(&claim_dir).is_ok());
    write_text_file(&claim_dir.join("job-1.req.json"), "one\n");
    assert_eq!(
        recover_shared_queue_job(&root, "job-1.req.json"),
        Err(SharedQueueRecoverError::MissingLease)
    );
}

#[test]
fn shared_queue_recovery_rejects_symlink_lease_directory_without_touching_target() {
    let root = clean_test_dir("shared-queue-recover-symlink-lease");
    let outside = clean_test_dir("shared-queue-recover-symlink-lease-outside");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    assert!(fs::remove_dir_all(root.join("lease").join(claimed.job_name())).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    write_text_file(&outside.join("worker"), "worker-a\n");
    assert!(symlink(&outside, root.join("lease").join(claimed.job_name())).is_ok());

    assert_eq!(
        recover_shared_queue_job(&root, claimed.job_name()),
        Err(SharedQueueRecoverError::MissingLease)
    );
    assert_file_text(&outside.join("worker"), "worker-a\n");
    assert!(!root.join("pending").join(claimed.job_name()).exists());
    assert!(claimed.claimed_path().exists());
}

#[test]
fn shared_queue_recovery_rejects_symlink_claim_directory_without_touching_target() {
    let root = clean_test_dir("shared-queue-recover-symlink-claim");
    let outside = clean_test_dir("shared-queue-recover-symlink-claim-outside");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    assert!(fs::remove_dir_all(root.join("claimed").join(claimed.job_name())).is_ok());
    write_text_file(&outside.join(claimed.job_name()), "outside\n");
    assert!(symlink(&outside, root.join("claimed").join(claimed.job_name())).is_ok());

    assert_eq!(
        recover_shared_queue_job(&root, claimed.job_name()),
        Err(SharedQueueRecoverError::MissingClaim)
    );
    assert_file_text(&outside.join(claimed.job_name()), "outside\n");
    assert!(!root.join("pending").join(claimed.job_name()).exists());
}
