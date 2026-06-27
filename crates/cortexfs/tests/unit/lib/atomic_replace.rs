#[test]
fn atomic_replace_text_with_mode_replaces_content_and_mode() -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("state.txt");
    fs::write(&path, "old")?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;

    atomic_replace_text_with_mode(&path, "new\n", 0o600)?;

    assert_eq!(fs::read_to_string(&path)?, "new\n");
    assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
    Ok(())
}

#[test]
fn atomic_replace_text_with_mode_rejects_symlink_parent_without_writing_target(
) -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let link_parent = temp.path().join("state");
    assert!(symlink(outside.path(), &link_parent).is_ok());

    let result = atomic_replace_text_with_mode(&link_parent.join("index"), "new\n", 0o600);

    assert!(result.is_err());
    assert!(!outside.path().join("index").exists());
    Ok(())
}

#[test]
fn atomic_replace_text_with_mode_rejects_symlink_intermediate_dir_without_writing_target(
) -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let outside_nested = outside.path().join("nested");
    fs::create_dir_all(&outside_nested)?;
    let outside_index = outside_nested.join("index");
    fs::write(&outside_index, "outside\n")?;
    let link_parent = temp.path().join("state");
    assert!(symlink(outside.path(), &link_parent).is_ok());

    let result = atomic_replace_text_with_mode(&link_parent.join("nested").join("index"), "new\n", 0o600);

    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&outside_index)?, "outside\n");
    Ok(())
}

#[test]
fn atomic_replace_text_with_mode_replaces_symlink_without_writing_target(
) -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let target = outside.path().join("secret");
    let link = temp.path().join("state.txt");
    fs::write(&target, "target\n")?;
    assert!(symlink(&target, &link).is_ok());

    atomic_replace_text_with_mode(&link, "new\n", 0o600)?;

    assert_eq!(fs::read_to_string(&target)?, "target\n");
    assert_eq!(fs::read_to_string(&link)?, "new\n");
    assert!(link.symlink_metadata()?.is_file());
    Ok(())
}

#[test]
fn append_jsonl_line_appends_newline_to_plain_file() -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("events.jsonl");
    fs::write(&path, "old\n")?;

    append_jsonl_line(&path, "{\"type\":\"done\"}")?;

    assert_eq!(fs::read_to_string(&path)?, "old\n{\"type\":\"done\"}\n");
    Ok(())
}

#[test]
fn append_jsonl_line_rejects_symlink_parent_without_writing_target() -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let link_parent = temp.path().join("session");
    let outside_events = outside.path().join("events.jsonl");
    fs::write(&outside_events, "outside\n")?;
    assert!(symlink(outside.path(), &link_parent).is_ok());

    let result = append_jsonl_line(&link_parent.join("events.jsonl"), "{\"type\":\"done\"}");

    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&outside_events)?, "outside\n");
    Ok(())
}

#[test]
fn append_jsonl_line_rejects_symlink_intermediate_dir_without_writing_target(
) -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let outside_session = outside.path().join("default");
    fs::create_dir_all(&outside_session)?;
    let outside_events = outside_session.join("events.jsonl");
    fs::write(&outside_events, "outside\n")?;
    let link_parent = temp.path().join("sessions");
    assert!(symlink(outside.path(), &link_parent).is_ok());

    let result = append_jsonl_line(
        &link_parent.join("default").join("events.jsonl"),
        "{\"type\":\"done\"}",
    );

    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&outside_events)?, "outside\n");
    Ok(())
}

#[test]
fn append_jsonl_line_refuses_non_regular_targets() {
    let result = append_jsonl_line(Path::new("/dev/null"), "{\"type\":\"done\"}");

    assert!(result.is_err());
}
