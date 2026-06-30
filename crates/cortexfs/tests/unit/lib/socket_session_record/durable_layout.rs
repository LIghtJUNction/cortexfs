#[test]
fn durable_session_layout_uses_private_modes_for_session_state() {
    let root = clean_test_dir("durable-session-private-modes");

    let result = ensure_durable_session_layout(
        &root,
        "default",
        "/work/project",
        Some("debug/echo"),
        SocketSessionScope::Private,
    );
    assert!(result.is_ok());

    let session = root.join("default");
    assert_eq!(
        fs::metadata(&session)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o700)
    );
    assert_eq!(
        fs::metadata(session.join("context"))
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o700)
    );
    for file in ["messages.jsonl", "events.jsonl", "meta.json", "cwd"] {
        assert_eq!(
            fs::metadata(session.join(file))
                .map(|metadata| metadata.permissions().mode() & 0o777)
                .ok(),
            Some(0o600),
            "{file}"
        );
    }
}

#[test]
fn durable_session_layout_rejects_session_symlink() {
    let root = clean_test_dir("durable-session-symlink");
    assert!(fs::create_dir_all(&root).is_ok());
    let target = clean_test_dir("durable-session-symlink-target");
    assert!(fs::create_dir_all(&target).is_ok());
    assert!(symlink(&target, root.join("default")).is_ok());

    let result = ensure_durable_session_layout(
        &root,
        "default",
        "/work/project",
        Some("debug/echo"),
        SocketSessionScope::Private,
    );

    assert_eq!(result, Err(DurableSessionLayoutError::CannotCreate));
    assert!(!target.join("messages.jsonl").exists());
}

#[test]
fn durable_session_layout_rejects_symlink_required_file_without_writing_target() {
    let root = clean_test_dir("durable-session-file-symlink");
    let outside = clean_test_dir("durable-session-file-symlink-target");
    let session_root = root.join("session");
    let session = session_root.join("default");
    let target = outside.join("messages.jsonl");
    assert!(fs::create_dir_all(&session).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(fs::write(&target, "outside\n").is_ok());
    assert!(symlink(&target, session.join("messages.jsonl")).is_ok());

    let result = ensure_durable_session_layout(
        &session_root,
        "default",
        "/work/project",
        Some("debug/echo"),
        SocketSessionScope::Private,
    );

    assert_eq!(result, Err(DurableSessionLayoutError::CannotCreate));
    assert_file_text(&target, "outside\n");
}

#[test]
fn durable_session_permission_helpers_repair_plain_file_and_dir_modes() {
    let root = clean_test_dir("durable-session-permission-repair");
    assert!(fs::create_dir_all(&root).is_ok());
    let file = root.join("state");
    let dir = root.join("context");
    write_text_file(&file, "idle\n");
    set_file_mode(&file, 0o644);
    assert!(fs::create_dir_all(&dir).is_ok());
    assert!(fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).is_ok());

    assert_eq!(set_text_file_permissions(&file), Ok(()));
    assert_eq!(set_private_dir_permissions(&dir), Ok(()));

    assert_eq!(
        fs::metadata(&file)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o600)
    );
    assert_eq!(
        fs::metadata(&dir)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o700)
    );
}

#[test]
fn durable_session_permission_helpers_refuse_symlinks_without_chmodding_targets() {
    let root = clean_test_dir("durable-session-permission-symlink");
    assert!(fs::create_dir_all(&root).is_ok());
    let outside = clean_test_dir("durable-session-permission-symlink-target");
    assert!(fs::create_dir_all(&outside).is_ok());
    let target_file = outside.join("state");
    let target_dir = outside.join("context");
    write_text_file(&target_file, "idle\n");
    set_file_mode(&target_file, 0o644);
    assert!(fs::create_dir_all(&target_dir).is_ok());
    assert!(fs::set_permissions(&target_dir, fs::Permissions::from_mode(0o755)).is_ok());
    let file_link = root.join("state");
    let dir_link = root.join("context");
    assert!(symlink(&target_file, &file_link).is_ok());
    assert!(symlink(&target_dir, &dir_link).is_ok());

    assert_eq!(
        set_text_file_permissions(&file_link),
        Err(DurableSessionLayoutError::CannotCreate)
    );
    assert_eq!(
        set_private_dir_permissions(&dir_link),
        Err(DurableSessionLayoutError::CannotCreate)
    );
    assert_eq!(
        fs::metadata(&target_file)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o644)
    );
    assert_eq!(
        fs::metadata(&target_dir)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o755)
    );
}

#[test]
fn durable_session_sync_plain_directory_refuses_symlink_without_touching_target() {
    let root = clean_test_dir("durable-session-sync-dir-symlink");
    assert!(fs::create_dir_all(&root).is_ok());
    let outside = clean_test_dir("durable-session-sync-dir-symlink-target");
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(fs::set_permissions(&outside, fs::Permissions::from_mode(0o755)).is_ok());
    let link = root.join("context");
    assert!(symlink(&outside, &link).is_ok());

    let result = sync_plain_directory(&link);

    assert!(result.is_err());
    assert_eq!(
        fs::metadata(&outside)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o755)
    );
}

#[test]
fn write_text_file_if_absent_repairs_plain_file_mode_without_replacing_content() {
    let root = clean_test_dir("write-absent-existing-plain");
    assert!(fs::create_dir_all(&root).is_ok());
    let path = root.join("result.md");
    write_text_file(&path, "existing\n");
    set_file_mode(&path, 0o644);

    let result = write_text_file_if_absent(&path, "new\n");

    assert!(result.is_ok());
    assert_file_text(&path, "existing\n");
    assert_eq!(
        fs::metadata(&path)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o600)
    );
}

#[test]
fn write_text_file_if_absent_refuses_symlink_without_chmodding_target() {
    let root = clean_test_dir("write-absent-symlink");
    assert!(fs::create_dir_all(&root).is_ok());
    let outside = clean_test_dir("write-absent-symlink-target");
    assert!(fs::create_dir_all(&outside).is_ok());
    let target = outside.join("target.md");
    write_text_file(&target, "target\n");
    set_file_mode(&target, 0o644);
    let link = root.join("result.md");
    assert!(symlink(&target, &link).is_ok());

    let result = write_text_file_if_absent(&link, "new\n");

    assert!(result.is_err());
    assert_file_text(&target, "target\n");
    assert_eq!(
        fs::metadata(&target)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o644)
    );
}

#[test]
fn write_text_file_if_absent_rejects_symlink_parent_without_writing_target() {
    let root = clean_test_dir("write-absent-symlink-parent");
    assert!(fs::create_dir_all(&root).is_ok());
    let outside = clean_test_dir("write-absent-symlink-parent-target");
    assert!(fs::create_dir_all(&outside).is_ok());
    let link = root.join("context");
    assert!(symlink(&outside, &link).is_ok());

    let result = write_text_file_if_absent(&link.join("result.md"), "new\n");

    assert!(result.is_err());
    assert!(!outside.join("result.md").exists());
}

#[test]
fn write_text_file_if_absent_rejects_symlink_parent_without_chmodding_existing_target() {
    let root = clean_test_dir("write-absent-symlink-parent-existing");
    assert!(fs::create_dir_all(&root).is_ok());
    let outside = clean_test_dir("write-absent-symlink-parent-existing-target");
    assert!(fs::create_dir_all(&outside).is_ok());
    let target = outside.join("result.md");
    write_text_file(&target, "target\n");
    set_file_mode(&target, 0o644);
    let link = root.join("context");
    assert!(symlink(&outside, &link).is_ok());

    let result = write_text_file_if_absent(&link.join("result.md"), "new\n");

    assert!(result.is_err());
    assert_file_text(&target, "target\n");
    assert_eq!(
        fs::metadata(&target)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o644)
    );
}

#[test]
fn write_text_file_if_absent_rejects_symlink_intermediate_parent_without_writing_target() {
    let root = clean_test_dir("write-absent-symlink-intermediate-parent");
    assert!(fs::create_dir_all(&root).is_ok());
    let outside = clean_test_dir("write-absent-symlink-intermediate-parent-target");
    assert!(fs::create_dir_all(outside.join("context")).is_ok());
    assert!(symlink(&outside, root.join("session")).is_ok());

    let result = write_text_file_if_absent(&root.join("session/context/result.md"), "new\n");

    assert!(result.is_err());
    assert!(!outside.join("context/result.md").exists());
}

#[test]
fn create_private_context_dir_repairs_plain_dir_mode() {
    let root = clean_test_dir("private-context-dir-existing");
    let path = root.join("context").join("child");
    assert!(fs::create_dir_all(&path).is_ok());
    assert!(fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).is_ok());

    let result = create_private_context_dir(&path);

    assert!(result.is_ok());
    assert_eq!(
        fs::metadata(&path)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o700)
    );
}

#[test]
fn create_private_context_dir_refuses_symlink_without_chmodding_target() {
    let root = clean_test_dir("private-context-dir-symlink");
    assert!(fs::create_dir_all(&root).is_ok());
    let outside = clean_test_dir("private-context-dir-symlink-target");
    let target = outside.join("target-dir");
    assert!(fs::create_dir_all(&target).is_ok());
    assert!(fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).is_ok());
    let link = root.join("child");
    assert!(symlink(&target, &link).is_ok());

    let result = create_private_context_dir(&link);

    assert!(result.is_err());
    assert_eq!(
        fs::metadata(&target)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o755)
    );
}

#[test]
fn create_private_context_dir_rejects_symlink_parent_without_writing_target() {
    let root = clean_test_dir("private-context-dir-symlink-parent");
    assert!(fs::create_dir_all(&root).is_ok());
    let outside = clean_test_dir("private-context-dir-symlink-parent-target");
    assert!(fs::create_dir_all(&outside).is_ok());
    let link = root.join("context");
    assert!(symlink(&outside, &link).is_ok());

    let result = create_private_context_dir(&link.join("child"));

    assert!(result.is_err());
    assert!(!outside.join("child").exists());
}

#[test]
fn create_private_context_dir_rejects_symlink_intermediate_parent_without_writing_target() {
    let root = clean_test_dir("private-context-dir-symlink-intermediate-parent");
    assert!(fs::create_dir_all(&root).is_ok());
    let outside = clean_test_dir("private-context-dir-symlink-intermediate-parent-target");
    assert!(fs::create_dir_all(outside.join("context")).is_ok());
    assert!(symlink(&outside, root.join("session")).is_ok());

    let result = create_private_context_dir(&root.join("session/context/child"));

    assert!(result.is_err());
    assert!(!outside.join("context/child").exists());
}
