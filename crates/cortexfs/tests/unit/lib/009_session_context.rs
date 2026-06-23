#[test]
fn child_context_recorder_rejects_bad_names_status_and_refs() {
    let root = unique_test_dir("child-context-record-bad");
    let session = root.join("default");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
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

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn session_layout_inspector_accepts_transparent_context_tree() {
    let root = unique_test_dir("session-layout-ok");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_session_layout(&root);

    let report = inspect_session_layout(&root);
    assert!(report.is_ok());
    assert!(report.issues().is_empty());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn session_layout_inspector_reports_missing_and_wrong_types() {
    let root = unique_test_dir("session-layout-bad");
    let context = root.join("context");
    let child = context.join("child").join("rev-1");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
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

    let _ignored = fs::remove_dir_all(&root);
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
        inspect_session_control(SessionControlKind::MetaJson, "{").issues(),
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
    let root = unique_test_dir("session-layout-control-bad");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
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

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn session_index_accepts_fixed_formats() {
    assert!(inspect_session_index(SessionIndexKind::List, "default\nreview-1\n").is_ok());
    assert!(inspect_session_index(SessionIndexKind::Current, "default\n").is_ok());
    assert!(inspect_session_index(SessionIndexKind::ByCwd, "worktree-1").is_ok());
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
    let root = unique_test_dir("session-index-update");
    let session_root = root.join("session");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());
    assert!(fs::create_dir_all(session_root.join("default")).is_ok());
    assert!(fs::create_dir_all(session_root.join("review-1")).is_ok());
    write_text_file(
        &session_root.join("index").join("list"),
        "default\nreview-1\n",
    );
    write_text_file(&session_root.join("index").join("current"), "default\n");

    let updated = update_session_index(&session_root, "review-1", Some("cwd-hash-1"));
    assert_eq!(updated, Ok(()));
    let list = fs::read_to_string(session_root.join("index").join("list"));
    assert!(list.is_ok());
    let Ok(list) = list else { return };
    let current = fs::read_to_string(session_root.join("index").join("current"));
    assert!(current.is_ok());
    let Ok(current) = current else { return };
    let by_cwd = fs::read_to_string(session_root.join("index").join("by-cwd").join("cwd-hash-1"));
    assert!(by_cwd.is_ok());
    let Ok(by_cwd) = by_cwd else { return };

    assert_eq!(list, "review-1\ndefault\n");
    assert_eq!(current, "review-1\n");
    assert_eq!(by_cwd, "review-1\n");
    assert!(inspect_session_index(SessionIndexKind::List, &list).is_ok());
    assert!(inspect_session_index(SessionIndexKind::Current, &current).is_ok());
    assert!(inspect_session_index(SessionIndexKind::ByCwd, &by_cwd).is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn session_index_update_rejects_missing_and_invalid_index_state() {
    let root = unique_test_dir("session-index-update-bad");
    let session_root = root.join("session");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
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
        update_session_index(&session_root, "default", None),
        Err(SessionIndexUpdateError::InvalidIndex)
    );
    assert_eq!(SessionIndexUpdateError::InvalidIndex.errno(), "EINVAL");

    let _ignored = fs::remove_dir_all(&root);
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
    let root = unique_test_dir("context-pack-rebuild");
    let session = root.join("default");
    let context = session.join("context");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
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
    assert!(built.is_ok());
    let Ok(built) = built else { return };

    let pack_json = fs::read_to_string(context.join("pack.json"));
    assert!(pack_json.is_ok());
    let Ok(pack_json) = pack_json else { return };
    let pack_md = fs::read_to_string(context.join("pack.md"));
    assert!(pack_md.is_ok());
    let Ok(pack_md) = pack_md else { return };

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

    let _ignored = fs::remove_dir_all(&root);
}

