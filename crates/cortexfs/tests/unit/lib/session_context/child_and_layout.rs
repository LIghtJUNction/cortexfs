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
        .contains(&PathLayoutIssue::wrong_kind("messages.jsonl".to_owned(), LayoutPathRole::File)));
    assert!(report
        .issues()
        .contains(&PathLayoutIssue::missing("events.jsonl".to_owned(), LayoutPathRole::File)));
    assert!(report
        .issues()
        .contains(&PathLayoutIssue::wrong_kind("context/pack.md".to_owned(), LayoutPathRole::File)));
    assert!(report.issues().contains(&PathLayoutIssue::missing("context/child/rev-1/result.md".to_owned(), LayoutPathRole::File)));
    assert!(report
        .issues()
        .contains(&PathLayoutIssue::missing("context/child/rev-1/artifact".to_owned(), LayoutPathRole::Directory)));
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
        .contains(&PathLayoutIssue::wrong_kind("state".to_owned(), LayoutPathRole::File)));
    assert!(!report
        .issues()
        .contains(&PathLayoutIssue::invalid_value("state".to_owned(), "running".to_owned())));
    assert!(report
        .issues()
        .contains(&PathLayoutIssue::wrong_kind("context".to_owned(), LayoutPathRole::Directory)));
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
        .contains(&PathLayoutIssue::wrong_kind(".".to_owned(), LayoutPathRole::Directory)));
    assert!(report
        .issues()
        .contains(&PathLayoutIssue::missing("state".to_owned(), LayoutPathRole::File)));
    assert!(!report
        .issues()
        .contains(&PathLayoutIssue::invalid_value("state".to_owned(), "running".to_owned())));
    assert_file_text(&outside.join("state"), "running\n");
}
