#[test]
fn terminal_safe_text_escapes_control_sequences() {
    let malicious = "prefix\u{1b}]52;c;payload\u{7}suffix";

    let rendered = terminal_safe_text(malicious);

    assert_eq!(rendered, "prefix\\u{1b}]52;c;payload\\u{7}suffix");
    assert!(!rendered.as_bytes().contains(&0x1b));
    assert!(!rendered.as_bytes().contains(&0x07));
}

#[test]
fn terminal_safe_text_preserves_common_formatting() {
    assert_eq!(terminal_safe_text("a\nb\tc\rd"), "a\nb\tc\rd");
}

#[test]
fn file_stat_xattr_line_escapes_terminal_controls() {
    let rendered = cortexfs_xattr_line("user.cortexfs.note\u{1b}[31m", "ok\u{7}");

    assert_eq!(rendered, "xattr.user.cortexfs.note\\u{1b}[31m=ok\\u{7}");
    assert!(!rendered.as_bytes().contains(&0x1b));
    assert!(!rendered.as_bytes().contains(&0x07));
}

#[test]
fn cli_error_line_escapes_terminal_controls() {
    let rendered = cli_error_line(&CliError::usage(
        "unexpected argument: --bad\u{1b}]52;c;payload\u{7}",
    ));

    assert_eq!(
        rendered,
        "ctx: unexpected argument: --bad\\u{1b}]52;c;payload\\u{7}"
    );
    assert!(!rendered.as_bytes().contains(&0x1b));
    assert!(!rendered.as_bytes().contains(&0x07));
}

#[test]
fn temp_file_name_changes_with_retry_attempt() {
    assert_ne!(temp_file_name(0), temp_file_name(1));
}

#[test]
fn provider_config_file_reader_refuses_symlink_targets() {
    let root = clean_test_dir("ctx-provider-config-reader-symlink");
    assert!(fs::create_dir_all(&root).is_ok());
    let target = root.join("target.json");
    let link = root.join("local.json");
    assert!(fs::write(&target, "{\"base_url\":\"http://127.0.0.1:8317/v1\"}\n").is_ok());
    assert!(std::os::unix::fs::symlink(&target, &link).is_ok());

    assert!(read_provider_config_file(&link).is_err());
}

#[test]
fn ctx_executable_open_refuses_symlink_targets() {
    let root = clean_test_dir("ctx-executable-open-symlink");
    assert!(fs::create_dir_all(&root).is_ok());
    let target = root.join("target");
    let link = root.join("tool");
    assert!(fs::write(&target, "#!/bin/sh\nexit 0\n").is_ok());
    assert!(fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).is_ok());
    assert!(std::os::unix::fs::symlink(&target, &link).is_ok());

    assert!(open_executable_no_follow(&link).is_err());
}

#[test]
fn provider_config_reader_lists_plain_config_dir() {
    let root = clean_test_dir("ctx-provider-config-reader-dir");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(
        fs::write(
            root.join("local.json"),
            "{\"name\":\"local\",\"base_url\":\"http://127.0.0.1:8317/v1\"}\n",
        )
        .is_ok()
    );

    assert!(read_provider_config_from_dir("local", &root).is_ok());
}

#[test]
fn provider_config_reader_rejects_symlink_config_dir() {
    let root = clean_test_dir("ctx-provider-config-reader-dir-symlink");
    let outside = clean_test_dir("ctx-provider-config-reader-dir-symlink-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(
        fs::write(
            outside.join("local.json"),
            "{\"name\":\"local\",\"base_url\":\"http://127.0.0.1:8317/v1\"}\n",
        )
        .is_ok()
    );
    assert!(std::os::unix::fs::symlink(&outside, root.join("providers.d")).is_ok());

    assert!(read_provider_config_from_dir("local", &root.join("providers.d")).is_err());
}

#[test]
fn provider_secret_stdin_reader_accepts_input_at_limit() {
    let secret = "x".repeat(MAX_PROVIDER_SECRET_STDIN_BYTES);

    let read = read_provider_secret_stdin_limited(
        std::io::Cursor::new(secret.as_bytes()),
        MAX_PROVIDER_SECRET_STDIN_BYTES,
    );

    assert_eq!(
        read.unwrap_or_default().len(),
        MAX_PROVIDER_SECRET_STDIN_BYTES
    );
}

#[test]
fn provider_secret_stdin_reader_rejects_input_over_limit() {
    let secret = "x".repeat(MAX_PROVIDER_SECRET_STDIN_BYTES + 1);

    let read = read_provider_secret_stdin_limited(
        std::io::Cursor::new(secret.as_bytes()),
        MAX_PROVIDER_SECRET_STDIN_BYTES,
    );

    assert!(matches!(read, Err(ref error) if error.kind() == std::io::ErrorKind::InvalidData));
}

#[test]
fn provider_config_file_reader_refuses_symlink_intermediate_directory() {
    let root = clean_test_dir("ctx-provider-config-reader-intermediate-symlink");
    let outside = clean_test_dir("ctx-provider-config-reader-intermediate-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(outside.join("providers.d")).is_ok());
    assert!(
        fs::write(
            outside.join("providers.d/local.json"),
            "{\"base_url\":\"http://127.0.0.1:8317/v1\"}\n",
        )
        .is_ok()
    );
    assert!(std::os::unix::fs::symlink(&outside, root.join("etc")).is_ok());

    assert!(read_provider_config_file(&root.join("etc/providers.d/local.json")).is_err());
}

#[test]
fn provider_config_atomic_write_replaces_file_without_fixed_temp_name() {
    let root = clean_test_dir("ctx-provider-config-atomic-write");
    assert!(fs::create_dir_all(&root).is_ok());
    let path = root.join("local.json");

    assert!(atomic_write_provider_config(&path, "{\"base_url\":\"http://old/v1\"}\n").is_ok());
    assert_eq!(
        fs::read_to_string(&path).unwrap_or_default(),
        "{\"base_url\":\"http://old/v1\"}\n"
    );
    assert!(atomic_write_provider_config(&path, "{\"base_url\":\"http://new/v1\"}\n").is_ok());
    assert_eq!(
        fs::read_to_string(&path).unwrap_or_default(),
        "{\"base_url\":\"http://new/v1\"}\n"
    );
    assert_eq!(
        fs::metadata(&path)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o600)
    );
    assert!(!root.join("local.json.tmp").exists());
}

#[test]
fn provider_config_atomic_write_rejects_symlink_parent_directory() {
    let root = clean_test_dir("ctx-provider-config-atomic-write-parent-symlink");
    let outside = clean_test_dir("ctx-provider-config-atomic-write-outside");
    assert!(fs::create_dir_all(root.join("etc")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("etc/providers.d")).is_ok());

    let path = root.join("etc/providers.d/local.json");

    assert!(atomic_write_provider_config(&path, "{\"base_url\":\"http://new/v1\"}\n").is_err());
    assert!(!outside.join("local.json").exists());
    assert!(!fs::read_dir(&outside).map_or(true, |entries| {
        entries
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().starts_with(".tmp"))
    }));
}

#[test]
fn provider_config_atomic_write_rejects_symlink_intermediate_directory() {
    let root = clean_test_dir("ctx-provider-config-atomic-write-intermediate-symlink");
    let outside = clean_test_dir("ctx-provider-config-atomic-write-intermediate-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(outside.join("providers.d")).is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("etc")).is_ok());

    let path = root.join("etc/providers.d/local.json");

    assert!(atomic_write_provider_config(&path, "{\"base_url\":\"http://new/v1\"}\n").is_err());
    assert!(!outside.join("providers.d/local.json").exists());
    assert!(
        !fs::read_dir(outside.join("providers.d")).map_or(true, |entries| {
            entries
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().starts_with(".tmp"))
        })
    );
}

#[test]
fn ctx_file_helpers_refuse_symlink_reads_and_appends() {
    let root = clean_test_dir("ctx-file-symlink-io");
    assert!(fs::create_dir_all(&root).is_ok());
    let target = root.join("outside.txt");
    let link = root.join("link.txt");
    assert!(fs::write(&target, "outside\n").is_ok());
    assert!(std::os::unix::fs::symlink(&target, &link).is_ok());

    assert!(cat_path(&link, None).is_err());
    assert!(read_file_to_string(&link).is_err());
    assert!(file_append(&root, "link.txt", "changed").is_err());
    assert_eq!(fs::read_to_string(&target).unwrap_or_default(), "outside\n");
}

#[test]
fn ctx_file_helpers_refuse_symlink_intermediate_reads() {
    let root = clean_test_dir("ctx-file-symlink-intermediate-read");
    let outside = clean_test_dir("ctx-file-symlink-intermediate-read-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(outside.join("session")).is_ok());
    assert!(fs::write(outside.join("session/state"), "outside\n").is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("link")).is_ok());
    let path = root.join("link/session/state");

    assert!(cat_path(&path, None).is_err());
    assert!(read_file_to_string(&path).is_err());
}

#[test]
fn ctx_file_type_refuses_symlink_intermediate_path() {
    let root = clean_test_dir("ctx-file-type-symlink-intermediate");
    let outside = clean_test_dir("ctx-file-type-symlink-intermediate-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(outside.join("session")).is_ok());
    assert!(fs::write(outside.join("session/state"), "outside\n").is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("link")).is_ok());

    assert!(file_type_name(&root, "link/session/state").is_err());
}

#[test]
fn ctx_file_writes_reject_symlink_parent_without_writing_target() {
    let root = clean_test_dir("ctx-file-symlink-parent-write");
    let outside = clean_test_dir("ctx-file-symlink-parent-write-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("link")).is_ok());

    assert!(file_set(&root, "link/state", "changed").is_err());
    assert!(file_append(&root, "link/events.jsonl", "{\"type\":\"changed\"}").is_err());
    assert!(!outside.join("state").exists());
    assert!(!outside.join("events.jsonl").exists());
}

#[test]
fn ctx_file_writes_reject_symlink_intermediate_parent_without_writing_target() {
    let root = clean_test_dir("ctx-file-symlink-intermediate-write");
    let outside = clean_test_dir("ctx-file-symlink-intermediate-write-outside");
    let outside_session = outside.join("session");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside_session).is_ok());
    assert!(fs::write(outside_session.join("state"), "outside\n").is_ok());
    assert!(fs::write(outside_session.join("events.jsonl"), "outside\n").is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("link")).is_ok());

    assert!(file_set(&root, "link/session/state", "changed").is_err());
    assert!(file_append(&root, "link/session/events.jsonl", "{\"type\":\"changed\"}").is_err());
    assert_eq!(
        fs::read_to_string(outside_session.join("state")).unwrap_or_default(),
        "outside\n"
    );
    assert_eq!(
        fs::read_to_string(outside_session.join("events.jsonl")).unwrap_or_default(),
        "outside\n"
    );
}

#[test]
fn ctx_file_set_and_append_refuse_session_history_without_side_effects() {
    let root = clean_test_dir("ctx-file-session-history-read-only");
    let paths = [
        "home/1000/agent/executor/session/default/messages.jsonl",
        "home/1000/model/openai/gpt-5.6.d/session/default/events.jsonl",
        "shared/team/agent/executor/session/default/events.jsonl",
        "shared/team/model/openai/gpt-5.6.d/session/default/messages.jsonl",
    ];

    for (index, path) in paths.into_iter().enumerate() {
        let marker = root.join(path);
        let session = marker.parent().unwrap_or(&marker);
        let store = session.join(".store");
        let claim = store.join("claim");
        let cursor = claim.join(".cursor.json");
        let claim_file = claim.join("claim-1");
        assert!(fs::create_dir_all(&claim).is_ok());
        assert!(fs::write(&marker, format!("marker-{index}\n")).is_ok());
        assert!(fs::write(&cursor, b"cursor-marker\n").is_ok());
        assert!(fs::write(&claim_file, b"claim-marker\n").is_ok());

        let snapshot = || {
            let mut store_entries = fs::read_dir(&store)
                .map(|entries| {
                    entries
                        .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            store_entries.sort();
            let mut claim_entries = fs::read_dir(&claim)
                .map(|entries| {
                    entries
                        .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            claim_entries.sort();
            (
                fs::read(&marker).ok(),
                fs::metadata(&marker).map(|metadata| metadata.ino()).ok(),
                fs::metadata(&store).map(|metadata| metadata.ino()).ok(),
                fs::metadata(&claim).map(|metadata| metadata.ino()).ok(),
                store_entries,
                claim_entries,
                fs::read(&cursor).ok(),
                fs::read(&claim_file).ok(),
            )
        };
        let before = snapshot();

        for operation in ["set", "append"] {
            let result = if operation == "set" {
                file_set(&root, path, "replacement")
            } else {
                file_append(&root, path, "replacement")
            };
            assert!(result.is_err(), "session history mutation must be refused");
            let Err(error) = result else {
                return;
            };
            assert_eq!(
                (error.code, error.message),
                (
                    2,
                    format!(
                        "session history is read-only; maintained by the runtime or an authorized FUSE writer: {path}"
                    )
                ),
                "unexpected {operation} error for {path}"
            );
            assert_eq!(snapshot(), before, "{operation} changed {path}");
        }
    }
}

#[test]
fn ctx_file_set_and_append_preserve_ordinary_file_semantics() {
    let root = clean_test_dir("ctx-file-ordinary-write");
    assert!(fs::create_dir_all(root.join("shared/project")).is_ok());
    assert!(file_set(&root, "shared/project/note", "first").is_ok());
    assert!(file_append(&root, "shared/project/note", "second").is_ok());
    assert_eq!(
        fs::read_to_string(root.join("shared/project/note")).ok(),
        Some("first\nsecond\n".to_owned())
    );
}

#[test]
fn ctx_agent_read_helpers_refuse_symlink_files() {
    let root = clean_test_dir("ctx-agent-read-helper-symlink");
    let outside = clean_test_dir("ctx-agent-read-helper-symlink-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    write_text_file(&outside.join("model"), "outside\n");
    assert!(std::os::unix::fs::symlink(outside.join("model"), root.join("model")).is_ok());

    assert!(read_optional_trimmed(&root.join("model")).is_err());
}
