#[test]
fn child_context_recorder_rejects_bad_names_status_and_refs() {
    let root = clean_test_dir("child-context-record-bad");
    let session = root.join("default");

    create_complete_session_layout(&session);

    assert_eq!(
        record_child_handoff_to_parent_context(
            &session,
            "bad/child",
            "reviewer",
            "default",
            "Task: no\n",
        ),
        Err(ChildContextRecordError::InvalidChildName)
    );
    assert_eq!(
        record_child_handoff_to_parent_context(
            &session,
            "rev-2",
            "reviewer",
            "default",
            "Task: no\n",
        ),
        Ok(())
    );
    assert_eq!(
        record_child_result_to_parent_context(
            &session,
            "rev-2",
            ChildContextStatus::Pending,
            "not terminal",
            "",
        ),
        Err(ChildContextRecordError::InvalidStatus)
    );
    assert_eq!(
        record_child_result_to_parent_context(
            &session,
            "rev-2",
            ChildContextStatus::Done,
            "done",
            "{\"path\":\"../secret\"}\n",
        ),
        Err(ChildContextRecordError::InvalidRefs)
    );
    assert_eq!(ChildContextRecordError::InvalidRefs.errno(), "EINVAL");
}

#[test]
fn session_layout_inspector_accepts_transparent_context_tree() {
    let root = clean_test_dir("session-layout-ok");
    create_complete_session_layout(&root);

    let report = inspect_session_layout(&root);
    assert!(report.is_ok());
    assert!(report.issues().is_empty());
}

#[test]
fn session_layout_inspector_reports_missing_and_wrong_types() {
    let root = clean_test_dir("session-layout-bad");
    let context = root.join("context");
    let child = context.join("child").join("rev-1");
    assert!(fs::create_dir_all(root.join("messages.jsonl")).is_ok());
    assert!(fs::create_dir_all(&child).is_ok());
    assert!(fs::write(child.join("agent"), "reviewer\n").is_ok());
    assert!(fs::create_dir_all(context.join("pack.md")).is_ok());

    let report = inspect_session_layout(&root);
    assert!(!report.is_ok());
    assert!(report
        .issues()
        .contains(&SessionLayoutIssue::NotFile("messages.jsonl".to_owned())));
    assert!(report
        .issues()
        .contains(&SessionLayoutIssue::MissingFile("events.jsonl".to_owned())));
    assert!(report
        .issues()
        .contains(&SessionLayoutIssue::NotFile("context/pack.md".to_owned())));
    assert!(report.issues().contains(&SessionLayoutIssue::MissingFile(
        "context/child/rev-1/result.md".to_owned()
    )));
    assert!(report
        .issues()
        .contains(&SessionLayoutIssue::MissingDirectory(
            "context/child/rev-1/artifact".to_owned()
        )));
}

#[test]
fn session_layout_inspector_rejects_symlink_files_and_directories_without_following() {
    let root = clean_test_dir("session-layout-symlink");
    let outside = clean_test_dir("session-layout-symlink-outside");
    create_complete_session_layout(&root);
    write_text_file(&outside.join("state"), "running\n");
    assert!(fs::remove_file(root.join("state")).is_ok());
    assert!(symlink(outside.join("state"), root.join("state")).is_ok());
    assert!(fs::remove_dir_all(root.join("context")).is_ok());
    assert!(fs::create_dir_all(outside.join("context")).is_ok());
    assert!(symlink(outside.join("context"), root.join("context")).is_ok());

    let report = inspect_session_layout(&root);

    assert!(report
        .issues()
        .contains(&SessionLayoutIssue::NotFile("state".to_owned())));
    assert!(!report
        .issues()
        .contains(&SessionLayoutIssue::InvalidFileValue {
            path: "state".to_owned(),
            value: "running".to_owned()
        }));
    assert!(report
        .issues()
        .contains(&SessionLayoutIssue::NotDirectory("context".to_owned())));
    assert_file_text(&outside.join("state"), "running\n");
}

#[test]
fn session_layout_inspector_rejects_symlink_session_root_without_following() {
    let root = clean_test_dir("session-layout-symlink-root");
    let outside = clean_test_dir("session-layout-symlink-root-outside");
    let link = root.join("default");
    create_complete_session_layout(&outside);
    write_text_file(&outside.join("state"), "running\n");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(symlink(&outside, &link).is_ok());

    let report = inspect_session_layout(&link);

    assert!(report
        .issues()
        .contains(&SessionLayoutIssue::NotDirectory(".".to_owned())));
    assert!(report
        .issues()
        .contains(&SessionLayoutIssue::MissingFile("state".to_owned())));
    assert!(!report
        .issues()
        .contains(&SessionLayoutIssue::InvalidFileValue {
            path: "state".to_owned(),
            value: "running".to_owned()
        }));
    assert_file_text(&outside.join("state"), "running\n");
}

#[test]
fn session_controls_accept_fixed_v1_values() {
    assert!(inspect_session_control(SessionControlKind::State, "active\n").is_ok());
    assert!(inspect_session_control(SessionControlKind::State, "cancelled\n").is_ok());
    assert!(inspect_session_control(SessionControlKind::Cwd, "/work/project\n").is_ok());
    assert!(inspect_session_control(
        SessionControlKind::MetaJson,
        "{\"client\":\"ctx\",\"model\":\"debug/echo\",\"scope\":\"shared\"}\n"
    )
    .is_ok());
    assert!(inspect_session_control(SessionControlKind::MetaJson, "{}\n").is_ok());
}

#[test]
fn session_controls_reject_invalid_state_cwd_and_meta() {
    assert_eq!(
        inspect_session_control(SessionControlKind::State, "running\n").issues(),
        &[SessionControlIssue::InvalidValue {
            line: 1,
            value: "running".to_owned()
        }]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::Cwd, "../work\n").issues(),
        &[SessionControlIssue::InvalidValue {
            line: 1,
            value: "../work".to_owned()
        }]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::Cwd, "/work/../secret\n").issues(),
        &[SessionControlIssue::InvalidValue {
            line: 1,
            value: "/work/../secret".to_owned()
        }]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::Cwd, "/work\rsecret\n").issues(),
        &[SessionControlIssue::InvalidValue {
            line: 1,
            value: "/work\rsecret".to_owned()
        }]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::MetaJson, "{").issues(),
        &[SessionControlIssue::InvalidJson]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::MetaJson, "{} trailing").issues(),
        &[SessionControlIssue::InvalidJson]
    );
    assert_eq!(
        inspect_session_control(
            SessionControlKind::MetaJson,
            "{\"client\":\"a\",\"client\":\"b\"}\n"
        )
        .issues(),
        &[SessionControlIssue::InvalidJson]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::MetaJson, "[]\n").issues(),
        &[SessionControlIssue::NotObject]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::MetaJson, "{\"scope\":\"global\"}\n").issues(),
        &[SessionControlIssue::InvalidValue {
            line: 1,
            value: "global".to_owned()
        }]
    );
}

#[test]
fn session_layout_inspector_rejects_invalid_control_values() {
    let root = clean_test_dir("session-layout-control-bad");
    create_complete_session_layout(&root);
    write_text_file(&root.join("state"), "running\n");
    write_text_file(&root.join("cwd"), "/work/../secret\n");
    write_text_file(&root.join("meta.json"), "{\"model\":\"bad/model/extra\"}\n");

    let report = inspect_session_layout(&root);
    assert!(report
        .issues()
        .contains(&SessionLayoutIssue::InvalidFileValue {
            path: "state".to_owned(),
            value: "running".to_owned()
        }));
    assert!(report
        .issues()
        .contains(&SessionLayoutIssue::InvalidFileValue {
            path: "cwd".to_owned(),
            value: "/work/../secret".to_owned()
        }));
    assert!(report
        .issues()
        .contains(&SessionLayoutIssue::InvalidFileValue {
            path: "meta.json".to_owned(),
            value: "bad/model/extra".to_owned()
        }));
}

#[test]
fn session_index_accepts_fixed_formats() {
    assert!(inspect_session_index(SessionIndexKind::List, "default\nreview-1\n").is_ok());
    assert!(inspect_session_index(SessionIndexKind::Current, "default\n").is_ok());
    assert!(inspect_session_index(SessionIndexKind::ByCwd, "worktree-1").is_ok());
    assert!(inspect_session_index(SessionIndexKind::ByHash, "hash-session").is_ok());
    assert!(inspect_session_index(SessionIndexKind::ByUuid, "uuid-session").is_ok());
    assert!(inspect_session_index(SessionIndexKind::List, "").is_ok());
}

#[test]
fn session_index_rejects_invalid_names_and_multi_value_files() {
    let list = inspect_session_index(SessionIndexKind::List, "default\nbad/name\n\n spaced\n");
    assert_eq!(
        list.issues(),
        &[
            SessionIndexIssue::InvalidSessionName {
                line: 2,
                value: "bad/name".to_owned()
            },
            SessionIndexIssue::EmptyValue { line: 3 },
            SessionIndexIssue::InvalidSessionName {
                line: 4,
                value: "spaced".to_owned()
            }
        ]
    );

    let current = inspect_session_index(SessionIndexKind::Current, "default\nother\n");
    assert_eq!(
        current.issues(),
        &[SessionIndexIssue::MultipleValues { line: 2 }]
    );

    let empty = inspect_session_index(SessionIndexKind::ByCwd, "");
    assert_eq!(empty.issues(), &[SessionIndexIssue::EmptyValue { line: 1 }]);
}

#[test]
fn session_index_update_sets_current_and_deduplicated_list() {
    let root = clean_test_dir("session-index-update");
    let session_root = root.join("session");
    assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());
    assert!(fs::create_dir_all(session_root.join("index").join("by-hash")).is_ok());
    assert!(fs::create_dir_all(session_root.join("index").join("by-uuid")).is_ok());
    assert!(fs::create_dir_all(session_root.join("default")).is_ok());
    assert!(fs::create_dir_all(session_root.join("review-1")).is_ok());
    write_text_file(
        &session_root.join("index").join("list"),
        "default\nreview-1\n",
    );
    write_text_file(&session_root.join("index").join("current"), "default\n");

    let updated = update_session_index_with_keys(
        &session_root,
        "review-1",
        Some("cwd-hash-1"),
        Some("content-hash-1"),
        Some("uuid-1"),
    );
    assert_eq!(updated, Ok(()));
    let list = fs::read_to_string(session_root.join("index").join("list"));
    let list = ok!(list);
    let current = fs::read_to_string(session_root.join("index").join("current"));
    let current = ok!(current);
    let by_cwd = fs::read_to_string(session_root.join("index").join("by-cwd").join("cwd-hash-1"));
    let by_cwd = ok!(by_cwd);
    let by_hash = fs::read_to_string(
        session_root
            .join("index")
            .join("by-hash")
            .join("content-hash-1"),
    );
    let by_hash = ok!(by_hash);
    let by_uuid = fs::read_to_string(session_root.join("index").join("by-uuid").join("uuid-1"));
    let by_uuid = ok!(by_uuid);

    assert_eq!(list, "review-1\ndefault\n");
    assert_eq!(current, "review-1\n");
    assert_eq!(by_cwd, "review-1\n");
    assert_eq!(by_hash, "review-1\n");
    assert_eq!(by_uuid, "review-1\n");
    assert!(inspect_session_index(SessionIndexKind::List, &list).is_ok());
    assert!(inspect_session_index(SessionIndexKind::Current, &current).is_ok());
    assert!(inspect_session_index(SessionIndexKind::ByCwd, &by_cwd).is_ok());
    assert!(inspect_session_index(SessionIndexKind::ByHash, &by_hash).is_ok());
    assert!(inspect_session_index(SessionIndexKind::ByUuid, &by_uuid).is_ok());
}

#[test]
fn session_index_update_rejects_missing_and_invalid_index_state() {
    let root = clean_test_dir("session-index-update-bad");
    let session_root = root.join("session");
    assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());
    assert!(fs::create_dir_all(session_root.join("default")).is_ok());
    write_text_file(&session_root.join("index").join("list"), "bad/name\n");
    write_text_file(&session_root.join("index").join("current"), "default\n");

    assert_eq!(
        update_session_index(&session_root, "bad/name", None),
        Err(SessionIndexUpdateError::InvalidSessionName)
    );
    assert_eq!(
        update_session_index(&session_root, "missing", None),
        Err(SessionIndexUpdateError::MissingSession)
    );
    assert_eq!(
        update_session_index(&session_root, "default", Some("bad/key")),
        Err(SessionIndexUpdateError::InvalidByCwdKey)
    );
    assert_eq!(
        update_session_index_with_keys(&session_root, "default", None, Some("bad/key"), None),
        Err(SessionIndexUpdateError::InvalidByHashKey)
    );
    assert_eq!(
        update_session_index_with_keys(&session_root, "default", None, None, Some("bad/key")),
        Err(SessionIndexUpdateError::InvalidByUuidKey)
    );
    assert_eq!(
        update_session_index(&session_root, "default", None),
        Err(SessionIndexUpdateError::InvalidIndex)
    );
    assert_eq!(SessionIndexUpdateError::InvalidIndex.errno(), "EINVAL");
}

#[test]
fn session_index_update_rejects_symlink_index_paths() {
    let root = clean_test_dir("session-index-update-symlink");
    let session_root = root.join("session");
    assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());
    assert!(fs::create_dir_all(session_root.join("default")).is_ok());
    write_text_file(&session_root.join("index").join("list"), "default\n");
    write_text_file(&session_root.join("index").join("current"), "default\n");

    let outside = clean_test_dir("session-index-update-symlink-outside");
    write_text_file(&outside.join("list"), "outside\n");
    assert!(fs::remove_file(session_root.join("index").join("list")).is_ok());
    assert!(symlink(outside.join("list"), session_root.join("index").join("list")).is_ok());

    assert_eq!(
        update_session_index(&session_root, "default", None),
        Err(SessionIndexUpdateError::MissingIndex)
    );
    assert_file_text(&outside.join("list"), "outside\n");
}

#[test]
fn session_index_update_rejects_symlink_intermediate_index_dir() {
    let root = clean_test_dir("session-index-update-symlink-index-dir");
    let session_root = root.join("session");
    let outside = clean_test_dir("session-index-update-symlink-index-dir-outside");
    assert!(fs::create_dir_all(&session_root).is_ok());
    assert!(fs::create_dir_all(session_root.join("default")).is_ok());
    assert!(fs::create_dir_all(outside.join("by-cwd")).is_ok());
    write_text_file(&outside.join("list"), "default\n");
    write_text_file(&outside.join("current"), "default\n");
    assert!(symlink(&outside, session_root.join("index")).is_ok());

    assert_eq!(
        update_session_index(&session_root, "default", None),
        Err(SessionIndexUpdateError::MissingIndex)
    );
    assert_file_text(&outside.join("list"), "default\n");
    assert_file_text(&outside.join("current"), "default\n");
}

#[test]
fn session_index_update_rejects_symlink_session_and_index_dirs() {
    let root = clean_test_dir("session-index-update-symlink-dirs");
    let session_root = root.join("session");
    let outside = clean_test_dir("session-index-update-symlink-dir-outside");
    assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());
    assert!(fs::create_dir_all(outside.join("default")).is_ok());
    write_text_file(&session_root.join("index").join("list"), "default\n");
    write_text_file(&session_root.join("index").join("current"), "default\n");
    assert!(symlink(outside.join("default"), session_root.join("default")).is_ok());

    assert_eq!(
        update_session_index(&session_root, "default", None),
        Err(SessionIndexUpdateError::MissingSession)
    );

    assert!(fs::remove_file(session_root.join("default")).is_ok());
    assert!(fs::create_dir_all(session_root.join("default")).is_ok());
    assert!(fs::remove_dir_all(session_root.join("index").join("by-cwd")).is_ok());
    assert!(symlink(&outside, session_root.join("index").join("by-cwd")).is_ok());

    assert_eq!(
        update_session_index(&session_root, "default", Some("cwd-hash-1")),
        Err(SessionIndexUpdateError::MissingIndex)
    );
}

#[test]
fn session_index_update_rejects_oversized_index_files() {
    let root = clean_test_dir("session-index-update-oversized");
    let session_root = root.join("session");
    assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());
    assert!(fs::create_dir_all(session_root.join("default")).is_ok());
    write_text_file(
        &session_root.join("index").join("list"),
        &"x".repeat((64 * 1024) + 1),
    );
    write_text_file(&session_root.join("index").join("current"), "default\n");

    assert_eq!(
        update_session_index(&session_root, "default", None),
        Err(SessionIndexUpdateError::CannotRecord)
    );
}

#[test]
fn context_pack_sources_are_session_relative_and_inspectable() {
    let report = inspect_context_pack_json(
        r#"{
  "session": "default",
  "agent": "coder",
  "items": [
{"kind": "summary", "source": "context/summary.md"},
{"kind": "messages", "source": "messages.jsonl"},
{"kind": "child_result", "source": "context/child/rev-1/result.md"},
{"kind": "child_refs", "source": "context/child/rev-1/refs.jsonl"},
{"kind": "artifact", "source": "context/child/rev-1/artifact/report.md"},
{"kind": "pinned", "source": "context/pinned/system.md"}
  ]
}"#,
    );
    assert!(report.is_ok());
    assert!(validate_context_pack_source("context/facts.jsonl").is_ok());
}

#[test]
fn context_pack_sources_reject_escapes_and_child_history() {
    assert_eq!(
        validate_context_pack_source("/ctx/shared/im-a/agent/bot/session/group-1/messages.jsonl"),
        Err(ContextPackSourceError::Absolute)
    );
    assert_eq!(
        validate_context_pack_source("../other/messages.jsonl"),
        Err(ContextPackSourceError::ParentComponent)
    );
    assert_eq!(
        validate_context_pack_source("session/other/messages.jsonl"),
        Err(ContextPackSourceError::UnsupportedSessionPath)
    );
    assert_eq!(
        validate_context_pack_source("context/child/rev-1/messages.jsonl"),
        Err(ContextPackSourceError::UnsupportedChildPath)
    );

    let report = inspect_context_pack_json(
        r#"{
  "items": [
{"kind": "ok", "source": "context/summary.md"},
{"kind": "absolute", "source": "/ctx/shared/im-b/agent/bot/session/channel-2/messages.jsonl"},
{"kind": "child_full_history", "source": "context/child/rev-1/messages.jsonl"},
{"kind": "missing"},
{"kind": "not_string", "source": 42}
  ]
}"#,
    );
    assert!(!report.is_ok());
    assert_eq!(
        report.issues(),
        [
            ContextPackIssue::InvalidSource {
                item: 1,
                source: "/ctx/shared/im-b/agent/bot/session/channel-2/messages.jsonl".to_owned(),
                reason: ContextPackSourceError::Absolute
            },
            ContextPackIssue::InvalidSource {
                item: 2,
                source: "context/child/rev-1/messages.jsonl".to_owned(),
                reason: ContextPackSourceError::UnsupportedChildPath
            },
            ContextPackIssue::MissingSource(3),
            ContextPackIssue::SourceNotString(4)
        ]
    );
}

#[test]
fn context_pack_rebuild_writes_inspectable_sources_without_child_history() {
    let root = clean_test_dir("context-pack-rebuild");
    let session = root.join("default");
    let context = session.join("context");

    create_complete_session_layout(&session);
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"system\",\"content\":\"base rules\"}\n{\"role\":\"user\",\"content\":\"fix tests\"}\n{\"role\":\"assistant\",\"content\":\"working\"}\n",
    );
    write_text_file(&context.join("budget"), "0\n");
    write_text_file(
        &context.join("pinned").join("system.md"),
        "Pinned system text\n",
    );
    write_text_file(&context.join("summary.md"), "Short summary\n");
    write_text_file(
        &context.join("facts.jsonl"),
        "{\"id\":\"f1\",\"text\":\"Root ABI is frozen.\",\"source\":\"messages:1-2\"}\n",
    );
    write_text_file(
        &context.join("decisions.jsonl"),
        "{\"id\":\"d1\",\"decision\":\"Do not add provider root.\",\"source\":\"messages:3\"}\n",
    );
    write_text_file(&context.join("todo.md"), "Keep FUSE small\n");
    write_text_file(
        &context.join("refs.jsonl"),
        "{\"id\":\"r1\",\"path\":\"docs/spec/16-context.md\",\"kind\":\"file\",\"summary\":\"context spec\"}\n",
    );
    write_text_file(
        &context.join("child").join("rev-1").join("result.md"),
        "Child says ok\n",
    );
    write_text_file(
        &context.join("child").join("rev-1").join("refs.jsonl"),
        "{\"id\":\"cr1\",\"path\":\"artifact/report.md\",\"kind\":\"artifact\",\"summary\":\"child report\"}\n",
    );
    write_text_file(
        &context.join("child").join("rev-1").join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"must not be packed\"}\n",
    );

    let built = rebuild_context_pack(&session, Some("coder"), 2);
    let built = ok!(built);

    let pack_json = fs::read_to_string(context.join("pack.json"));
    let pack_json = ok!(pack_json);
    let pack_md = fs::read_to_string(context.join("pack.md"));
    let pack_md = ok!(pack_md);

    assert_eq!(built.pack_json(), pack_json);
    assert_eq!(built.pack_md(), pack_md);
    assert!(inspect_context_pack_json(&pack_json).is_ok());
    assert!(pack_json.contains("\"source\":\"context/pinned/system.md\""));
    assert!(pack_json.contains("\"source\":\"messages.jsonl\""));
    assert!(pack_json.contains("\"range\":\"tail:2\""));
    assert!(pack_json.contains("\"source\":\"context/child/rev-1/result.md\""));
    assert!(pack_json.contains("\"source\":\"context/child/rev-1/refs.jsonl\""));
    assert!(!pack_json.contains("context/child/rev-1/messages.jsonl"));
    assert!(pack_md.contains("Pinned system text"));
    assert!(pack_md.contains("Child says ok"));
    assert!(pack_md.contains("\"role\":\"assistant\""));
    assert!(!pack_md.contains("must not be packed"));
    assert!(built.items().iter().all(|item| {
        validate_context_pack_source(item.source()).is_ok()
            && item.source() != "context/child/rev-1/messages.jsonl"
    }));
}
