#[test]
fn session_controls_accept_fixed_v1_values() {
    assert!(inspect_session_control(SessionControlKind::State, "active\n").is_ok());
    assert!(inspect_session_control(SessionControlKind::State, "cancelled\n").is_ok());
    assert!(inspect_session_control(SessionControlKind::Cwd, "/work/project\n").is_ok());
    assert!(
        inspect_session_control(
            SessionControlKind::MetaJson,
            "{\"client\":\"ctx\",\"model\":\"debug/echo\",\"scope\":\"shared\"}\n"
        )
        .is_ok()
    );
    assert!(inspect_session_control(SessionControlKind::MetaJson, "{}\n").is_ok());
}

#[test]
fn session_controls_reject_invalid_state_cwd_and_meta() {
    assert_eq!(
        inspect_session_control(SessionControlKind::State, "running\n").issues(),
        &[ControlLineIssue::InvalidValue {
            line: 1,
            value: "running".to_owned()
        }]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::Cwd, "../work\n").issues(),
        &[ControlLineIssue::InvalidValue {
            line: 1,
            value: "../work".to_owned()
        }]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::Cwd, "/work/../secret\n").issues(),
        &[ControlLineIssue::InvalidValue {
            line: 1,
            value: "/work/../secret".to_owned()
        }]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::Cwd, "/work\rsecret\n").issues(),
        &[ControlLineIssue::InvalidValue {
            line: 1,
            value: "/work\rsecret".to_owned()
        }]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::MetaJson, "{").issues(),
        &[ControlLineIssue::InvalidJson]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::MetaJson, "{} trailing").issues(),
        &[ControlLineIssue::InvalidJson]
    );
    assert_eq!(
        inspect_session_control(
            SessionControlKind::MetaJson,
            "{\"client\":\"a\",\"client\":\"b\"}\n"
        )
        .issues(),
        &[ControlLineIssue::InvalidJson]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::MetaJson, "[]\n").issues(),
        &[ControlLineIssue::NotObject]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::MetaJson, "{\"scope\":\"global\"}\n").issues(),
        &[ControlLineIssue::InvalidValue {
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
    assert!(report.issues().contains(&PathLayoutIssue::invalid_value(
        "state".to_owned(),
        "running".to_owned()
    )));
    assert!(report.issues().contains(&PathLayoutIssue::invalid_value(
        "cwd".to_owned(),
        "/work/../secret".to_owned()
    )));
    assert!(report.issues().contains(&PathLayoutIssue::invalid_value(
        "meta.json".to_owned(),
        "bad/model/extra".to_owned()
    )));
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
            ControlLineIssue::InvalidValue {
                line: 2,
                value: "bad/name".to_owned()
            },
            ControlLineIssue::EmptyValue { line: 3 },
            ControlLineIssue::InvalidValue {
                line: 4,
                value: "spaced".to_owned()
            }
        ]
    );

    let current = inspect_session_index(SessionIndexKind::Current, "default\nother\n");
    assert_eq!(
        current.issues(),
        &[ControlLineIssue::MultipleValues { line: 2 }]
    );

    let empty = inspect_session_index(SessionIndexKind::ByCwd, "");
    assert_eq!(empty.issues(), &[ControlLineIssue::EmptyValue { line: 1 }]);
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
fn session_index_update_preserves_existing_file_metadata() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let root = clean_test_dir("session-index-update-metadata");
    let session_root = root.join("session");
    let index = session_root.join("index");
    let secondary = index.join("by-cwd/cwd-hash-1");
    assert!(fs::create_dir_all(index.join("by-cwd")).is_ok());
    assert!(fs::create_dir_all(session_root.join("default")).is_ok());
    write_text_file(&index.join("list"), "default\n");
    write_text_file(&index.join("current"), "default\n");
    write_text_file(&secondary, "old\n");
    for path in [&index.join("list"), &index.join("current"), &secondary] {
        assert!(fs::set_permissions(path, fs::Permissions::from_mode(0o640)).is_ok());
    }
    let metadata = |path: &Path| {
        fs::metadata(path)
            .map(|value| (value.uid(), value.gid(), value.permissions().mode() & 0o777))
            .ok()
    };
    let before = [
        metadata(&index.join("list")),
        metadata(&index.join("current")),
        metadata(&secondary),
    ];

    assert_eq!(
        update_session_index(&session_root, "default", Some("cwd-hash-1")),
        Ok(())
    );

    assert_eq!(
        [
            metadata(&index.join("list")),
            metadata(&index.join("current")),
            metadata(&secondary),
        ],
        before
    );
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
    assert!(
        symlink(
            outside.join("list"),
            session_root.join("index").join("list")
        )
        .is_ok()
    );

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
use super::*;
