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
    write_text_file(
        &outside.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"outside\"}\n",
    );
    assert!(fs::remove_file(session.join("messages.jsonl")).is_ok());
    assert!(
        symlink(
            outside.join("messages.jsonl"),
            session.join("messages.jsonl")
        )
        .is_ok()
    );
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
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"ok\"}\n",
    );
    write_text_file(&context.join("budget"), "0\n");
    write_text_file(&context.join("summary.md"), "");
    write_text_file(&context.join("facts.jsonl"), "");
    write_text_file(&context.join("decisions.jsonl"), "");
    write_text_file(&context.join("todo.md"), "");
    write_text_file(&context.join("refs.jsonl"), "");
    write_text_file(&context.join("child").join("rev-1").join("result.md"), "");
    write_text_file(&context.join("child").join("rev-1").join("refs.jsonl"), "");
    write_text_file(&outside.join("system.md"), "outside pinned\n");
    assert!(
        symlink(
            outside.join("system.md"),
            context.join("pinned").join("system.md")
        )
        .is_ok()
    );

    let built = rebuild_context_pack(&session, Some("coder"), 5);
    let built = ok!(built);
    assert!(!built.pack_md().contains("outside pinned"));
    assert!(
        !built
            .items()
            .iter()
            .any(|item| item.source() == "context/pinned/system.md")
    );
}

#[test]
fn context_pack_rebuild_ignores_control_character_pinned_files() {
    let root = clean_test_dir("context-pack-rebuild-control-pinned");
    let session = root.join("default");
    let context = session.join("context");

    create_complete_session_layout(&session);
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"ok\"}\n",
    );
    write_text_file(&context.join("budget"), "0\n");
    write_text_file(&context.join("summary.md"), "");
    write_text_file(&context.join("facts.jsonl"), "");
    write_text_file(&context.join("decisions.jsonl"), "");
    write_text_file(&context.join("todo.md"), "");
    write_text_file(&context.join("refs.jsonl"), "");
    write_text_file(&context.join("child").join("rev-1").join("result.md"), "");
    write_text_file(&context.join("child").join("rev-1").join("refs.jsonl"), "");
    write_text_file(
        &context.join("pinned").join("bad\u{1b}.md"),
        "hidden pinned\n",
    );

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
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"ok\"}\n",
    );
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
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"ok\"}\n",
    );
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
    write_text_file(
        &session.join("messages.jsonl"),
        &"x".repeat((1024 * 1024) + 1),
    );
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
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"ok\"}\n",
    );
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
use super::*;
