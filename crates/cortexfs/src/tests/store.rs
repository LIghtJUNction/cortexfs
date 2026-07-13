static LEGACY_SNAPSHOT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn session_history_stream_maps_only_abi_marker_paths() {
    assert_eq!(
        super::columnar::Stream::from_abi_path(
            "home/1000/agent/coder/session/default/messages.jsonl",
        ),
        Some(super::columnar::Stream::Messages),
    );
    assert_eq!(
        super::columnar::Stream::from_abi_path(
            "shared/team/agent/coder/session/default/events.jsonl",
        ),
        Some(super::columnar::Stream::Events),
    );
    assert_eq!(
        (
            super::columnar::Stream::from_abi_path(
                "home/1000/agent/coder/session/default/latest.md",
            ),
            super::columnar::Stream::from_abi_path("messages.jsonl"),
        ),
        (None, None),
    );
}

#[test]
fn legacy_history_read_does_not_create_store_on_read_only_session() {
    let root = reference_tree("legacy-history-read-only");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let messages = "{\"role\":\"user\",\"content\":\"hello\"}\n";
    write_text_file(&session.join("messages.jsonl"), messages);
    assert!(fs::set_permissions(&session, fs::Permissions::from_mode(0o555)).is_ok());
    assert!(
        fs::set_permissions(
            session.join("messages.jsonl"),
            fs::Permissions::from_mode(0o444),
        )
        .is_ok()
    );

    let history = ok!(super::columnar::read_text(
        &session,
        super::columnar::Stream::Messages,
        1024,
    ));
    assert_eq!(history, messages);
    assert!(!session.join(".store").exists());
    assert!(fs::set_permissions(&session, fs::Permissions::from_mode(0o700)).is_ok());
}

fn assert_legacy_snapshot_rechecks_created_lock(store_exists: bool) {
    use std::sync::{Arc, Barrier};

    let _serial = match LEGACY_SNAPSHOT_TEST_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    let case = if store_exists {
        "existing-store"
    } else {
        "missing-store"
    };
    let root = reference_tree(&format!("legacy-history-snapshot-lock-{case}"));
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let old_message = r#"{"role":"user","run":"old","content":"old"}"#;
    let old_event = r#"{"type":"start","id":"old","run":"old","scope":"private","cwd":"/work"}"#;
    let new_message = r#"{"role":"assistant","run":"new","content":"new"}"#;
    let new_event = r#"{"type":"done","run":"new","status":"ok"}"#;
    assert!(fs::write(session.join("messages.jsonl"), format!("{old_message}\n")).is_ok());
    assert!(fs::write(session.join("events.jsonl"), format!("{old_event}\n")).is_ok());
    if store_exists {
        assert!(fs::create_dir_all(session.join(".store")).is_ok());
    }

    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    super::columnar::set_legacy_snapshot_barrier(Some((
        session.clone(),
        Arc::clone(&entered),
        Arc::clone(&release),
    )));
    let reader_session = session.clone();
    let reader = thread::spawn(move || -> std::io::Result<(String, String)> {
        let guard = super::columnar::HistoryGuard::shared(&reader_session)?;
        Ok((
            guard.read_text(super::columnar::Stream::Messages, 1024)?,
            guard.read_text(super::columnar::Stream::Events, 1024)?,
        ))
    });
    entered.wait();

    let writer_session = session;
    let writer = thread::spawn(move || -> std::io::Result<()> {
        let guard = super::columnar::HistoryGuard::exclusive(&writer_session)?;
        guard.append(super::columnar::Stream::Messages, &[new_message])?;
        guard.append(super::columnar::Stream::Events, &[new_event])
    });
    assert!(writer.join().is_ok_and(|result| result.is_ok()));
    release.wait();
    let snapshot = reader.join().ok().and_then(Result::ok);
    super::columnar::set_legacy_snapshot_barrier(None);

    assert_eq!(
        snapshot,
        Some((
            format!("{old_message}\n{new_message}\n"),
            format!("{old_event}\n{new_event}\n"),
        )),
    );
}

#[test]
fn legacy_history_snapshot_rechecks_lock_created_with_store() {
    assert_legacy_snapshot_rechecks_created_lock(false);
}

#[test]
fn legacy_history_snapshot_rechecks_lock_created_in_existing_store() {
    assert_legacy_snapshot_rechecks_created_lock(true);
}

fn assert_legacy_append_rejects_changed_marker(replace: bool) {
    let case = if replace { "replace" } else { "truncate" };
    let root = reference_tree(&format!("legacy-history-append-{case}"));
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let messages = session.join("messages.jsonl");
    let original = r#"{"role":"user","run":"first","content":"hello"}"#;
    let start = r#"{"type":"start","id":"first","run":"first","scope":"private","cwd":"/work"}"#;
    assert!(fs::write(&messages, format!("{original}\n")).is_ok());
    assert!(fs::write(session.join("events.jsonl"), format!("{start}\n")).is_ok());
    let guard = ok!(super::columnar::HistoryGuard::exclusive(&session));

    if replace {
        let replacement = session.join("replacement.jsonl");
        assert!(fs::write(&replacement, b"replacement\n").is_ok());
        assert!(fs::rename(&replacement, &messages).is_ok());
    } else {
        let truncated = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&messages);
        assert!(truncated.is_ok_and(|file| file.sync_all().is_ok()));
    }
    let changed = fs::read(&messages).unwrap_or_default();

    let refresh = guard.refresh_claims();
    let append = guard.append(
        super::columnar::Stream::Messages,
        &[r#"{"role":"assistant","run":"first","content":"must-not-write"}"#],
    );

    assert!(refresh.is_err_and(|error| error.kind() == std::io::ErrorKind::InvalidData));
    assert!(append.is_err_and(|error| error.kind() == std::io::ErrorKind::InvalidData));
    assert_eq!(fs::read(&messages).unwrap_or_default(), changed);
    assert!(!session.join(".store/claim/.cursor.json").exists());
}

#[test]
fn legacy_history_guard_rejects_replaced_marker_before_claim_or_append() {
    assert_legacy_append_rejects_changed_marker(true);
}

#[test]
fn legacy_history_guard_rejects_truncated_marker_before_claim_or_append() {
    assert_legacy_append_rejects_changed_marker(false);
}

#[test]
fn legacy_history_guard_sequential_append_is_immediately_complete() {
    let root = reference_tree("legacy-history-sequential-append");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let user = r#"{"role":"user","run":"first","content":"hello"}"#;
    let start = r#"{"type":"start","id":"first","run":"first","scope":"private","cwd":"/work"}"#;
    let guard = ok!(super::columnar::HistoryGuard::exclusive(&session));

    ok!(guard.append(super::columnar::Stream::Messages, &[user]));
    ok!(guard.append(super::columnar::Stream::Events, &[start]));

    assert_eq!(
        (
            guard.len(super::columnar::Stream::Messages).ok(),
            guard
                .read_text(super::columnar::Stream::Messages, 1024)
                .ok(),
            guard.len(super::columnar::Stream::Events).ok(),
            guard.read_text(super::columnar::Stream::Events, 1024).ok(),
        ),
        (
            Some(u64::try_from(user.len() + 1).unwrap_or(u64::MAX)),
            Some(format!("{user}\n")),
            Some(u64::try_from(start.len() + 1).unwrap_or(u64::MAX)),
            Some(format!("{start}\n")),
        ),
    );
    assert!(matches!(
        guard.lookup_send("first", "hello", "private", Some("/work")),
        Ok(super::columnar::SendClaim::Replay(ref frame)) if frame == start
    ));
}

#[test]
fn legacy_history_guard_keeps_snapshot_across_first_columnar_writer() {
    let root = reference_tree("legacy-history-first-writer-snapshot");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let old = r#"{"role":"user","run":"old","content":"legacy"}"#;
    let new = r#"{"role":"assistant","run":"new","content":"committed"}"#;
    let old_history = format!("{old}\n");
    assert!(fs::write(session.join("messages.jsonl"), &old_history).is_ok());
    let guard = ok!(super::columnar::HistoryGuard::shared(&session));
    assert!(!session.join(".store").exists());

    let writer_session = session.clone();
    let writer = thread::spawn(move || {
        super::columnar::append(&writer_session, super::columnar::Stream::Messages, &[new])
    });
    assert!(matches!(writer.join(), Ok(Ok(()))));
    let committed = format!("{old}\n{new}\n");
    assert_eq!(
        super::columnar::read_text(
            &session,
            super::columnar::Stream::Messages,
            u64::try_from(committed.len()).unwrap_or(u64::MAX),
        )
        .ok(),
        Some(committed),
    );
    assert_eq!(
        (
            guard.is_columnar(),
            guard.len(super::columnar::Stream::Messages).ok(),
            guard
                .read_at(super::columnar::Stream::Messages, 0, old_history.len(),)
                .ok(),
            guard
                .tail(super::columnar::Stream::Messages, old_history.len())
                .ok(),
            guard
                .read_text(
                    super::columnar::Stream::Messages,
                    u64::try_from(old_history.len()).unwrap_or(u64::MAX),
                )
                .ok(),
        ),
        (
            false,
            Some(u64::try_from(old_history.len()).unwrap_or(u64::MAX)),
            Some(old_history.as_bytes().to_vec()),
            Some(old_history.as_bytes().to_vec()),
            Some(old_history),
        ),
    );
}

#[test]
fn session_store_columnar_guard_does_not_fall_back_when_manifest_disappears() {
    let root = reference_tree("session-store-columnar-guard-manifest");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &[r#"{"role":"user","run":"first","content":"committed"}"#],
    ));
    let guard = ok!(super::columnar::HistoryGuard::shared(&session));
    assert!(guard.is_columnar());
    assert!(fs::remove_file(session.join(".store/manifest.json")).is_ok());

    assert!(
        guard
            .len(super::columnar::Stream::Messages)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::InvalidData)
    );
    assert!(
        guard
            .read_at(super::columnar::Stream::Messages, 0, 1)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::InvalidData)
    );
    assert!(
        guard
            .tail(super::columnar::Stream::Messages, 1)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::InvalidData)
    );
}

#[test]
fn session_claim_index_scans_legacy_history_once() {
    let root = reference_tree("session-claim-legacy-once");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"run\":\"claim-1\",\"content\":\"hello\"}\n",
    );
    write_text_file(
        &session.join("events.jsonl"),
        "{\"type\":\"start\",\"id\":\"claim-1\",\"run\":\"claim-1\",\"scope\":\"private\",\"cwd\":\"/work\"}\n",
    );
    super::columnar::reset_claim_scan_bytes();
    let guard = ok!(super::columnar::HistoryGuard::exclusive(&session));

    assert!(matches!(
        guard.lookup_send("claim-1", "hello", "private", Some("/work")),
        Ok(super::columnar::SendClaim::Replay(ref frame))
            if frame.contains("\"type\":\"start\"")
    ));
    assert!(super::columnar::claim_scan_bytes() > 0);
    super::columnar::reset_claim_scan_bytes();
    assert!(matches!(
        guard.lookup_send("claim-1", "hello", "private", Some("/work")),
        Ok(super::columnar::SendClaim::Replay(_))
    ));
    assert_eq!(super::columnar::claim_scan_bytes(), 0);
}

#[test]
fn session_claim_cursor_replays_idempotently_after_cursor_failure() {
    let root = reference_tree("session-claim-cursor-recovery");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"run\":\"claim-1\",\"content\":\"hello\"}\n",
    );
    write_text_file(
        &session.join("events.jsonl"),
        "{\"type\":\"start\",\"id\":\"claim-1\",\"run\":\"claim-1\",\"scope\":\"private\",\"cwd\":\"/work\"}\n",
    );
    let guard = ok!(super::columnar::HistoryGuard::exclusive(&session));
    super::columnar::set_claim_cursor_failure(true);

    assert!(guard.refresh_claims().is_err());
    assert!(matches!(
        guard.lookup_send("claim-1", "hello", "private", Some("/work")),
        Ok(super::columnar::SendClaim::Replay(_))
    ));
}

#[test]
fn session_claim_cursor_survives_columnar_migration() {
    let root = reference_tree("session-claim-columnar-migration");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"run\":\"claim-1\",\"content\":\"hello\"}\n",
    );
    write_text_file(
        &session.join("events.jsonl"),
        "{\"type\":\"start\",\"id\":\"claim-1\",\"run\":\"claim-1\",\"scope\":\"private\",\"cwd\":\"/work\"}\n",
    );
    {
        let guard = ok!(super::columnar::HistoryGuard::exclusive(&session));
        assert!(matches!(
            guard.lookup_send("claim-1", "hello", "private", Some("/work")),
            Ok(super::columnar::SendClaim::Replay(_))
        ));
    }
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Events,
        &["{\"type\":\"usage\",\"run\":\"claim-1\",\"input_tokens\":1}"],
    ));
    let guard = ok!(super::columnar::HistoryGuard::exclusive(&session));

    assert!(matches!(
        guard.lookup_send("claim-1", "hello", "private", Some("/work")),
        Ok(super::columnar::SendClaim::Replay(_))
    ));
}

#[test]
fn session_store_recovers_durable_wal_as_exact_jsonl() {
    let root = reference_tree("session-store-wal");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let line = r#"{"role":"user","content":"hello"}"#;

    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &[line],
    ));

    let actual = ok!(super::columnar::read_at(
        &session,
        super::columnar::Stream::Messages,
        0,
        line.len() + 1,
    ));
    assert_eq!(actual, format!("{line}\n").as_bytes());
}

#[test]
fn session_store_flush_writes_readable_parquet_schema() {
    use arrow_array::{StringArray, UInt64Array};
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    let root = reference_tree("session-store-parquet");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let line = r#"{"role":"assistant","run":"run-1","content":"ok"}"#;
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &[line],
    ));

    ok!(super::columnar::flush(&session));

    let file = ok!(fs::File::open(
        session.join(".store/data/part-000000.parquet")
    ));
    let builder = ok!(ParquetRecordBatchReaderBuilder::try_new(file));
    let mut reader = ok!(builder.build());
    let batch = ok!(reader.next().unwrap_or_else(|| {
        Err(arrow_schema::ArrowError::InvalidArgumentError(
            "missing parquet batch".to_owned(),
        ))
    }));
    let stream = batch
        .column(0)
        .as_any()
        .downcast_ref::<StringArray>()
        .map(|array| array.value(0));
    let ordinal = batch
        .column(1)
        .as_any()
        .downcast_ref::<UInt64Array>()
        .map(|array| array.value(0));
    let payload = batch
        .column(5)
        .as_any()
        .downcast_ref::<StringArray>()
        .map(|array| array.value(0));
    assert_eq!(
        (stream, ordinal, payload),
        (Some("messages"), Some(0), Some(line))
    );
}

#[test]
fn session_store_read_at_crosses_parquet_shards() {
    let root = reference_tree("session-store-cross-shard");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let lines = (0..130)
        .map(|ordinal| format!(r#"{{"type":"delta","ordinal":{ordinal}}}"#))
        .collect::<Vec<_>>();
    let line_refs = lines.iter().map(String::as_str).collect::<Vec<_>>();
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Events,
        &line_refs,
    ));
    ok!(super::columnar::flush(&session));
    let expected = format!("{}\n", lines.join("\n"));
    let boundary = lines
        .iter()
        .take(128)
        .map(|line| line.len() + 1)
        .sum::<usize>();
    let offset = boundary - 7;

    let result = super::columnar::read_at(
        &session,
        super::columnar::Stream::Events,
        u64::try_from(offset).unwrap_or(0),
        31,
    );
    assert!(result.is_ok(), "{result:?}");
    let actual = result.unwrap_or_default();
    assert_eq!(
        actual,
        expected
            .as_bytes()
            .get(offset..offset + 31)
            .unwrap_or_default()
    );
}

#[test]
fn session_store_first_write_migrates_legacy_jsonl() {
    let root = reference_tree("session-store-legacy");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let old_message = r#"{"role":"user","content":"legacy"}"#;
    let old_event = r#"{"type":"start","run":"old"}"#;
    assert!(fs::write(session.join("messages.jsonl"), format!("{old_message}\n")).is_ok());
    assert!(fs::write(session.join("events.jsonl"), format!("{old_event}\n")).is_ok());
    let new_event = r#"{"type":"done","run":"new","status":"ok"}"#;

    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Events,
        &[new_event],
    ));

    let messages = ok!(super::columnar::read_at(
        &session,
        super::columnar::Stream::Messages,
        0,
        1024,
    ));
    let events = ok!(super::columnar::read_at(
        &session,
        super::columnar::Stream::Events,
        0,
        1024,
    ));
    let markers = (
        fs::metadata(session.join("messages.jsonl")).map_or(u64::MAX, |value| value.len()),
        fs::metadata(session.join("events.jsonl")).map_or(u64::MAX, |value| value.len()),
    );
    assert_eq!(
        (messages, events, markers),
        (
            format!("{old_message}\n").into_bytes(),
            format!("{old_event}\n{new_event}\n").into_bytes(),
            (0, 0),
        )
    );
}

#[test]
fn session_store_tail_is_bounded_and_keeps_recent_lines() {
    let root = reference_tree("session-store-tail");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let lines = (0..260)
        .map(|ordinal| format!(r#"{{"role":"user","ordinal":{ordinal}}}"#))
        .collect::<Vec<_>>();
    let first = lines
        .get(..259)
        .unwrap_or_default()
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &first,
    ));
    ok!(super::columnar::flush(&session));
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &[lines.get(259).map_or("", String::as_str)],
    ));
    let projection = format!("{}\n", lines.join("\n"));
    let start = projection.len().saturating_sub(64);
    let mut expected = projection
        .as_bytes()
        .get(start..)
        .unwrap_or_default()
        .to_vec();
    if start > 0
        && let Some(newline) = expected.iter().position(|byte| *byte == b'\n')
    {
        expected.drain(..=newline);
    }

    let actual = ok!(super::columnar::tail(
        &session,
        super::columnar::Stream::Messages,
        64,
    ));
    assert_eq!(actual, expected);
}

#[test]
fn session_store_export_writes_readable_parquet_dataset() {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    let root = reference_tree("session-store-export");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let lines = (0..300)
        .map(|ordinal| format!(r#"{{"type":"message","ordinal":{ordinal}}}"#))
        .collect::<Vec<_>>();
    let line_refs = lines.iter().map(String::as_str).collect::<Vec<_>>();
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Events,
        &line_refs,
    ));
    let output = root.join("dataset");

    ok!(super::columnar::export(&session, &output));

    let files = ok!(fs::read_dir(&output))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    let mut rows = 0_i64;
    for path in &files {
        let builder = ok!(ParquetRecordBatchReaderBuilder::try_new(ok!(
            fs::File::open(path)
        )));
        rows += builder.metadata().file_metadata().num_rows();
    }
    assert_eq!(
        (
            files.len(),
            files
                .iter()
                .all(|path| path.extension().and_then(|value| value.to_str()) == Some("parquet")),
            rows,
        ),
        (3, true, 300),
    );
}

#[test]
fn session_store_append_during_prune_keeps_both_rows() {
    use std::sync::{Arc, Barrier, mpsc};

    let root = reference_tree("session-store-concurrent-prune");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let first = r#"{"role":"user","content":"first"}"#;
    let second = r#"{"role":"user","content":"second"}"#;
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &[first],
    ));
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    super::columnar::set_prune_barriers(Some((Arc::clone(&entered), Arc::clone(&release))));
    let flush_session = session.clone();
    let flush = thread::spawn(move || super::columnar::flush(&flush_session));
    entered.wait();
    let append_session = session.clone();
    let (sender, receiver) = mpsc::channel();
    let append = thread::spawn(move || {
        let result = super::columnar::append(
            &append_session,
            super::columnar::Stream::Messages,
            &[second],
        );
        let _ignored = sender.send(result);
    });
    let early = receiver.recv_timeout(Duration::from_millis(250)).ok();
    release.wait();
    assert!(flush.join().is_ok_and(|result| result.is_ok()));
    let append_result = early.unwrap_or_else(|| {
        receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or_else(|error| Err(std::io::Error::other(error)))
    });
    assert!(append_result.is_ok());
    assert!(append.join().is_ok());
    super::columnar::set_prune_barriers(None);

    let actual = ok!(super::columnar::read_at(
        &session,
        super::columnar::Stream::Messages,
        0,
        1024,
    ));
    assert_eq!(actual, format!("{first}\n{second}\n").as_bytes());
}

#[test]
fn session_store_ignores_torn_wal_tail_and_recovers_on_append() {
    use std::fs::OpenOptions;

    let root = reference_tree("session-store-torn-wal");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let first = r#"{"role":"user","content":"first"}"#;
    let second = r#"{"role":"assistant","content":"second"}"#;
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &[first],
    ));
    let mut wal = ok!(OpenOptions::new()
        .append(true)
        .open(session.join(".store/wal.jsonl")));
    assert!(wal.write_all(br#"{"stream":"messages""#).is_ok());
    assert!(wal.sync_all().is_ok());

    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &[second],
    ));

    let actual = ok!(super::columnar::read_at(
        &session,
        super::columnar::Stream::Messages,
        0,
        1024,
    ));
    assert_eq!(actual, format!("{first}\n{second}\n").as_bytes());
}

#[test]
fn session_store_wal_staging_without_manifest_keeps_raw_history_authoritative() {
    let root = reference_tree("session-store-wal-staging");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let prefix = r#"{"role":"user","run":"first","content":"valid"}"#;
    let raw = format!("{prefix}\n{{\"role\":\"assistant\"");
    assert!(fs::write(session.join("messages.jsonl"), &raw).is_ok());
    let migration = super::columnar::append(
        &session,
        super::columnar::Stream::Events,
        &[r#"{"type":"usage","run":"first","input_tokens":1}"#],
    );
    assert!(migration.is_err_and(|error| error.kind() == std::io::ErrorKind::InvalidData));
    assert!(!session.join(".store/manifest.json").exists());
    assert!(fs::metadata(session.join(".store/wal.jsonl")).is_ok_and(|meta| meta.len() > 0));

    let guard = ok!(super::columnar::HistoryGuard::shared(&session));
    assert_eq!(
        (
            guard.is_columnar(),
            guard.len(super::columnar::Stream::Messages).ok(),
            guard
                .read_at(super::columnar::Stream::Messages, 0, raw.len())
                .ok(),
            guard
                .tail(super::columnar::Stream::Messages, raw.len())
                .ok(),
            guard
                .read_text(
                    super::columnar::Stream::Messages,
                    u64::try_from(raw.len()).unwrap_or(u64::MAX),
                )
                .ok(),
        ),
        (
            false,
            Some(u64::try_from(raw.len()).unwrap_or(u64::MAX)),
            Some(raw.as_bytes().to_vec()),
            Some(raw.as_bytes().to_vec()),
            Some(raw.clone()),
        ),
    );
    assert_eq!(
        (
            super::columnar::len(&session, super::columnar::Stream::Messages).ok(),
            super::columnar::read_at(&session, super::columnar::Stream::Messages, 0, raw.len(),)
                .ok(),
            super::columnar::tail(&session, super::columnar::Stream::Messages, raw.len(),).ok(),
            super::columnar::read_text(
                &session,
                super::columnar::Stream::Messages,
                u64::try_from(raw.len()).unwrap_or(u64::MAX),
            )
            .ok(),
        ),
        (
            Some(u64::try_from(raw.len()).unwrap_or(u64::MAX)),
            Some(raw.as_bytes().to_vec()),
            Some(raw.as_bytes().to_vec()),
            Some(raw),
        ),
    );
}

#[test]
fn session_store_wal_staging_keeps_guard_append_and_claims_on_raw_history() {
    let root = reference_tree("session-store-wal-staging-raw-append");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let user = r#"{"role":"user","run":"first","content":"hello"}"#;
    let start = r#"{"type":"start","id":"first","run":"first","scope":"private","cwd":"/work"}"#;
    let raw_events = format!("{start}\n{{\"type\":\"usage\"");
    assert!(fs::write(session.join("messages.jsonl"), format!("{user}\n")).is_ok());
    assert!(fs::write(session.join("events.jsonl"), &raw_events).is_ok());
    let migration = super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &[r#"{"role":"assistant","run":"first","content":"unused"}"#],
    );
    assert!(migration.is_err_and(|error| error.kind() == std::io::ErrorKind::InvalidData));
    assert!(!session.join(".store/manifest.json").exists());
    let wal_before = fs::read(session.join(".store/wal.jsonl")).unwrap_or_default();
    assert!(!wal_before.is_empty());

    let usage = r#"{"type":"usage","run":"first","input_tokens":1}"#;
    let guard = ok!(super::columnar::HistoryGuard::exclusive(&session));
    assert!(!guard.is_columnar());
    ok!(guard.append(super::columnar::Stream::Events, &[usage]));

    assert_eq!(
        fs::read(session.join(".store/wal.jsonl")).unwrap_or_default(),
        wal_before,
    );
    assert_eq!(
        fs::read_to_string(session.join("events.jsonl")).ok(),
        Some(format!("{raw_events}{usage}\n")),
    );
    assert!(
        guard
            .lookup_send("first", "hello", "private", Some("/work"))
            .is_err_and(|error| error.kind() == std::io::ErrorKind::InvalidData)
    );
}

#[test]
fn session_store_rejects_invalid_committed_wal_frame() {
    use std::fs::OpenOptions;

    let root = reference_tree("session-store-invalid-wal");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Events,
        &[r#"{"type":"start"}"#],
    ));
    let mut wal = ok!(OpenOptions::new()
        .append(true)
        .open(session.join(".store/wal.jsonl")));
    assert!(wal.write_all(b"not-json\n").is_ok());
    assert!(wal.sync_all().is_ok());

    let result = super::columnar::read_at(&session, super::columnar::Stream::Events, 0, 1024);
    assert!(result.is_err_and(|error| error.kind() == std::io::ErrorKind::InvalidData));
}

#[test]
fn session_store_rejects_payload_with_newline() {
    let root = reference_tree("session-store-newline-payload");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");

    let result = super::columnar::append(&session, super::columnar::Stream::Messages, &["{}\n{}"]);
    assert!(result.is_err_and(|error| error.kind() == std::io::ErrorKind::InvalidInput));
}

#[test]
fn session_store_rejects_invalid_batch_before_creating_or_appending() {
    let root = reference_tree("session-store-invalid-batch-atomic");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let messages_before = fs::read(session.join("messages.jsonl")).unwrap_or_default();
    let events_before = fs::read(session.join("events.jsonl")).unwrap_or_default();

    let result = super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &[r#"{"role":"user","content":"valid"}"#, "{}\r"],
    );

    assert!(result.is_err_and(|error| error.kind() == std::io::ErrorKind::InvalidInput));
    assert_eq!(
        (
            fs::read(session.join("messages.jsonl")).unwrap_or_default(),
            fs::read(session.join("events.jsonl")).unwrap_or_default(),
            session.join(".store").exists(),
        ),
        (messages_before, events_before, false),
    );
}

#[test]
fn session_store_accepts_256kib_payload_and_rejects_next_byte() {
    let root = reference_tree("session-store-row-boundary");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let accepted = format!("\"{}\"", r"\\".repeat((256 * 1024 - 2) / 2));
    let rejected = format!("\"{}\"", "a".repeat(256 * 1024 - 1));

    let first = super::columnar::append(&session, super::columnar::Stream::Messages, &[&accepted]);
    let second = super::columnar::append(&session, super::columnar::Stream::Messages, &[&rejected]);
    assert_eq!(
        (
            first.is_ok(),
            second.is_err_and(|error| error.kind() == std::io::ErrorKind::InvalidInput),
        ),
        (true, true),
    );
}

#[test]
fn session_store_manifest_uses_fixed_width_indexes() {
    let root = reference_tree("session-store-fixed-index");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &[r#"{"role":"user","content":"indexed"}"#],
    ));
    ok!(super::columnar::flush(&session));

    let manifest = ok!(fs::read_to_string(session.join(".store/manifest.json")));
    let value = ok!(serde_json::from_str::<serde_json::Value>(&manifest));
    let index_len = ok!(fs::metadata(session.join(".store/index/messages.idx"))).len();
    assert_eq!(
        (
            value.get("shards").is_none(),
            value.get("next_shard").and_then(serde_json::Value::as_u64),
            value
                .pointer("/messages/index_records")
                .and_then(serde_json::Value::as_u64),
            index_len,
        ),
        (true, Some(1), Some(1), 48),
    );
}

#[test]
fn session_store_tail_reads_index_in_logarithmic_time() {
    let root = reference_tree("session-store-index-logarithmic");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let lines = (0..4096)
        .map(|ordinal| format!(r#"{{"ordinal":{ordinal}}}"#))
        .collect::<Vec<_>>();
    let line_refs = lines.iter().map(String::as_str).collect::<Vec<_>>();
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &line_refs,
    ));
    ok!(super::columnar::flush(&session));
    super::columnar::reset_read_counters();

    let actual = ok!(super::columnar::tail(
        &session,
        super::columnar::Stream::Messages,
        64,
    ));
    let (index_reads, shard_opens) = super::columnar::read_counters();

    let last = lines.get(4095).map_or("", String::as_str);
    assert!(actual.ends_with(format!("{last}\n").as_bytes()));
    assert!(index_reads <= 8, "read {index_reads} index records");
    assert_eq!(shard_opens, 1);
}

#[test]
fn session_store_ignores_orphan_index_tail_and_shard() {
    use std::fs::OpenOptions;

    let root = reference_tree("session-store-orphan-index");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let line = r#"{"role":"user","content":"committed"}"#;
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &[line],
    ));
    ok!(super::columnar::flush(&session));
    let store = session.join(".store");
    let mut index = ok!(OpenOptions::new()
        .append(true)
        .open(store.join("index/messages.idx")));
    assert!(index.write_all(&[0x55; 48]).is_ok());
    assert!(index.sync_all().is_ok());
    assert!(
        fs::copy(
            store.join("data/part-000000.parquet"),
            store.join("data/part-000001.parquet"),
        )
        .is_ok()
    );

    let actual = ok!(super::columnar::read_at(
        &session,
        super::columnar::Stream::Messages,
        0,
        1024,
    ));
    assert_eq!(actual, format!("{line}\n").as_bytes());
}

#[test]
fn session_store_deduplicates_committed_rows_left_in_wal() {
    use std::fs::OpenOptions;

    let root = reference_tree("session-store-committed-wal");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let first = r#"{"role":"user","content":"first"}"#;
    let second = r#"{"role":"assistant","content":"second"}"#;
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &[first],
    ));
    ok!(super::columnar::flush(&session));
    let stale = serde_json::json!({
        "stream": "messages",
        "ordinal": 0,
        "payload": first,
    });
    let mut wal = ok!(OpenOptions::new()
        .append(true)
        .open(session.join(".store/wal.jsonl")));
    assert!(writeln!(wal, "{stale}").is_ok());
    assert!(wal.sync_all().is_ok());
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &[second],
    ));

    let actual = ok!(super::columnar::read_at(
        &session,
        super::columnar::Stream::Messages,
        0,
        1024,
    ));
    assert_eq!(actual, format!("{first}\n{second}\n").as_bytes());
}

#[test]
fn session_store_tightens_store_permissions() {
    let root = reference_tree("session-store-permissions");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let store = session.join(".store");
    assert!(fs::create_dir_all(store.join("data")).is_ok());
    assert!(fs::create_dir_all(store.join("index")).is_ok());
    for directory in [&store, &store.join("data"), &store.join("index")] {
        assert!(fs::set_permissions(directory, fs::Permissions::from_mode(0o755)).is_ok());
    }
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &[r#"{"role":"user","content":"private"}"#],
    ));
    ok!(super::columnar::flush(&session));

    for directory in [&store, &store.join("data"), &store.join("index")] {
        assert_eq!(
            ok!(fs::metadata(directory)).permissions().mode() & 0o777,
            0o700
        );
    }
    for file in [
        store.join("lock"),
        store.join("wal.jsonl"),
        store.join("manifest.json"),
        store.join("index/messages.idx"),
        store.join("index/events.idx"),
        store.join("data/part-000000.parquet"),
    ] {
        assert_eq!(ok!(fs::metadata(file)).permissions().mode() & 0o777, 0o600);
    }
}

#[test]
fn session_store_rejects_symlinked_storage_paths() {
    for kind in ["store", "data", "index", "wal"] {
        let name = format!("session-store-symlink-{kind}");
        let root = reference_tree(&name);
        let session_root = root.join("home/1000/agent/coder/session");
        ok!(ensure_durable_session_layout(
            &session_root,
            "default",
            "/work",
            Some("main"),
            SocketSessionScope::Private,
        ));
        let session = session_root.join("default");
        let store = session.join(".store");
        let outside = root.join(format!("outside-{kind}"));
        let result = match kind {
            "store" => {
                assert!(fs::create_dir_all(&outside).is_ok());
                assert!(symlink(&outside, &store).is_ok());
                super::columnar::append(&session, super::columnar::Stream::Messages, &["{}"])
            }
            "data" | "index" => {
                assert!(fs::create_dir_all(&store).is_ok());
                assert!(fs::create_dir_all(&outside).is_ok());
                assert!(symlink(&outside, store.join(kind)).is_ok());
                ok!(super::columnar::append(
                    &session,
                    super::columnar::Stream::Messages,
                    &["{}"],
                ));
                super::columnar::flush(&session)
            }
            "wal" => {
                assert!(fs::create_dir_all(&store).is_ok());
                assert!(fs::write(&outside, "").is_ok());
                assert!(symlink(&outside, store.join("wal.jsonl")).is_ok());
                super::columnar::append(&session, super::columnar::Stream::Messages, &["{}"])
            }
            _ => Err(std::io::Error::other("invalid test storage kind")),
        };
        assert!(
            result.is_err_and(|error| error.kind() == std::io::ErrorKind::InvalidData),
            "accepted symlinked {kind}"
        );
    }
}

#[test]
fn session_store_export_preserves_existing_output() {
    let root = reference_tree("session-store-export-existing");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &[r#"{"role":"user","content":"export"}"#],
    ));
    let output = root.join("dataset");
    assert!(fs::create_dir_all(&output).is_ok());
    assert!(fs::write(output.join("keep"), "unchanged").is_ok());

    let result = super::columnar::export(&session, &output);

    assert!(result.is_err());
    assert_eq!(ok!(fs::read_to_string(output.join("keep"))), "unchanged");
    assert_eq!(ok!(fs::read_dir(&output)).count(), 1);
}

#[test]
fn session_store_export_copy_failure_is_atomic_and_retryable() {
    let root = reference_tree("session-store-export-failure");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let lines = (0..200)
        .map(|ordinal| format!(r#"{{"ordinal":{ordinal}}}"#))
        .collect::<Vec<_>>();
    let line_refs = lines.iter().map(String::as_str).collect::<Vec<_>>();
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Events,
        &line_refs,
    ));
    let output = root.join("dataset");
    super::columnar::set_export_copy_failure(true);

    let failed = super::columnar::export(&session, &output);
    let leaked_temp = ok!(fs::read_dir(&*root))
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(".dataset.export-"))
        });

    assert!(failed.is_err());
    assert!(!output.exists());
    assert!(!leaked_temp);
    ok!(super::columnar::export(&session, &output));
    assert_eq!(ok!(fs::read_dir(output)).count(), 2);
}

#[test]
fn session_store_export_rejects_symlinked_parent() {
    let root = reference_tree("session-store-export-parent");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &["{}"],
    ));
    let real_parent = root.join("real-parent");
    let linked_parent = root.join("linked-parent");
    assert!(fs::create_dir_all(&real_parent).is_ok());
    assert!(symlink(&real_parent, &linked_parent).is_ok());

    let result = super::columnar::export(&session, &linked_parent.join("dataset"));

    assert!(result.is_err());
    assert!(!real_parent.join("dataset").exists());
}

#[test]
fn session_store_auto_flushes_on_row_threshold() {
    let root = reference_tree("session-store-auto-row-flush");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let lines = (0..128)
        .map(|ordinal| format!(r#"{{"ordinal":{ordinal}}}"#))
        .collect::<Vec<_>>();
    let first = lines
        .get(..127)
        .unwrap_or_default()
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &first,
    ));
    assert!(!session.join(".store/data/part-000000.parquet").exists());

    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &[lines.get(127).map_or("", String::as_str)],
    ));

    assert!(session.join(".store/data/part-000000.parquet").is_file());
    assert_eq!(ok!(fs::metadata(session.join(".store/wal.jsonl"))).len(), 0);
}

#[test]
fn session_store_auto_flushes_on_payload_threshold() {
    let root = reference_tree("session-store-auto-byte-flush");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let payload = format!("\"{}\"", "a".repeat(256 * 1024 - 2));
    let lines = (0..16).map(|_index| payload.as_str()).collect::<Vec<_>>();

    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Events,
        &lines,
    ));

    assert!(session.join(".store/data/part-000000.parquet").is_file());
    assert_eq!(ok!(fs::metadata(session.join(".store/wal.jsonl"))).len(), 0);
}

#[test]
fn session_store_large_append_leaves_bounded_wal() {
    let root = reference_tree("session-store-bounded-wal");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let lines = (0..1000)
        .map(|ordinal| format!(r#"{{"ordinal":{ordinal}}}"#))
        .collect::<Vec<_>>();
    let refs = lines.iter().map(String::as_str).collect::<Vec<_>>();

    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &refs,
    ));

    let wal = ok!(fs::read_to_string(session.join(".store/wal.jsonl")));
    assert!(wal.lines().count() < 128);
    assert!(wal.len() < 4 * 1024 * 1024);
}

#[test]
fn session_store_unique_temps_survive_stale_files_and_failures() {
    for failure in ["write", "shard-rename", "prune-rename"] {
        let root = reference_tree(&format!("session-store-temp-{failure}"));
        let session_root = root.join("home/1000/agent/coder/session");
        ok!(ensure_durable_session_layout(
            &session_root,
            "default",
            "/work",
            Some("main"),
            SocketSessionScope::Private,
        ));
        let session = session_root.join("default");
        let store = session.join(".store");
        assert!(fs::create_dir_all(store.join("data")).is_ok());
        assert!(
            fs::write(
                store.join(format!(".part-000000.parquet.tmp-{}", std::process::id())),
                "stale",
            )
            .is_ok()
        );
        assert!(
            fs::write(
                store.join(format!(".wal.jsonl.tmp-{}", std::process::id())),
                "stale",
            )
            .is_ok()
        );
        let lines = (0..128)
            .map(|ordinal| format!(r#"{{"ordinal":{ordinal}}}"#))
            .collect::<Vec<_>>();
        let refs = lines.iter().map(String::as_str).collect::<Vec<_>>();
        super::columnar::set_temp_failures(
            failure == "write",
            failure == "shard-rename",
            failure == "prune-rename",
        );

        let failed = super::columnar::append(&session, super::columnar::Stream::Messages, &refs);
        let leaked = ok!(fs::read_dir(&store))
            .filter_map(Result::ok)
            .any(|entry| {
                entry.file_name().to_str().is_some_and(|name| {
                    name.contains(".parquet.shard-") || name.starts_with(".wal.jsonl.prune-")
                })
            });
        assert!(failed.is_err(), "did not inject {failure}");
        assert!(!leaked, "leaked temp after {failure}");
        super::columnar::set_temp_failures(false, false, false);
        ok!(super::columnar::flush(&session));
        let actual = ok!(super::columnar::read_at(
            &session,
            super::columnar::Stream::Messages,
            0,
            64 * 1024,
        ));
        assert_eq!(actual, format!("{}\n", lines.join("\n")).as_bytes());
    }
}

#[test]
fn session_store_fixed_index_offset_boundaries() {
    let root = reference_tree("session-store-index-boundaries");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let lines = (0..256)
        .map(|ordinal| format!(r#"{{"ordinal":{ordinal}}}"#))
        .collect::<Vec<_>>();
    let refs = lines.iter().map(String::as_str).collect::<Vec<_>>();
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &refs,
    ));
    let boundary = lines
        .get(..128)
        .unwrap_or_default()
        .iter()
        .map(|line| line.len() + 1)
        .sum::<usize>();

    super::columnar::reset_read_counters();
    let start = ok!(super::columnar::read_at(
        &session,
        super::columnar::Stream::Messages,
        0,
        lines.first().map_or(0, |line| line.len() + 1),
    ));
    let start_counts = super::columnar::read_counters();
    super::columnar::reset_read_counters();
    let next = ok!(super::columnar::read_at(
        &session,
        super::columnar::Stream::Messages,
        u64::try_from(boundary).unwrap_or(0),
        lines.get(128).map_or(0, |line| line.len() + 1),
    ));
    let next_counts = super::columnar::read_counters();
    let length = ok!(super::columnar::len(
        &session,
        super::columnar::Stream::Messages,
    ));
    super::columnar::reset_read_counters();
    let eof = ok!(super::columnar::read_at(
        &session,
        super::columnar::Stream::Messages,
        length,
        32,
    ));
    let eof_counts = super::columnar::read_counters();

    assert_eq!(
        start,
        format!("{}\n", lines.first().map_or("", String::as_str)).as_bytes()
    );
    assert_eq!(
        next,
        format!("{}\n", lines.get(128).map_or("", String::as_str)).as_bytes()
    );
    assert!(start_counts.0 <= 3 && start_counts.1 == 1);
    assert!(next_counts.0 <= 3 && next_counts.1 == 1);
    assert!(eof.is_empty());
    assert_eq!(eof_counts, (0, 0));
}

#[test]
fn session_store_fixed_indexes_keep_streams_independent() {
    let root = reference_tree("session-store-index-streams");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");
    let messages = [r#"{"role":"user"}"#, r#"{"role":"assistant"}"#];
    let events = [r#"{"type":"start"}"#, r#"{"type":"done"}"#];
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &messages[..1],
    ));
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Events,
        &events,
    ));
    ok!(super::columnar::append(
        &session,
        super::columnar::Stream::Messages,
        &messages[1..],
    ));
    ok!(super::columnar::flush(&session));

    let actual_messages = ok!(super::columnar::read_at(
        &session,
        super::columnar::Stream::Messages,
        0,
        1024,
    ));
    let actual_events = ok!(super::columnar::read_at(
        &session,
        super::columnar::Stream::Events,
        0,
        1024,
    ));
    assert_eq!(
        actual_messages,
        format!("{}\n", messages.join("\n")).as_bytes()
    );
    assert_eq!(actual_events, format!("{}\n", events.join("\n")).as_bytes());
}

fn spawn_session_store_lock_child(
    session: &Path,
    socket: &Path,
    mode: &str,
) -> std::io::Result<(std::process::Child, UnixStream)> {
    use std::process::{Command, Stdio};

    let listener = UnixListener::bind(socket)?;
    let child = Command::new(std::env::current_exe()?)
        .arg("--exact")
        .arg("tests::store::session_store_lock_child")
        .arg("--nocapture")
        .env("CORTEXFS_LOCK_CHILD_MODE", mode)
        .env("CORTEXFS_LOCK_CHILD_SESSION", session)
        .env("CORTEXFS_LOCK_CHILD_SOCKET", socket)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let (stream, _address) = listener.accept()?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    Ok((child, stream))
}

fn expect_session_store_lock_byte(stream: &mut UnixStream, expected: u8) -> std::io::Result<()> {
    let mut byte = [0_u8; 1];
    stream.read_exact(&mut byte)?;
    if byte == [expected] {
        Ok(())
    } else {
        Err(std::io::Error::other("unexpected lock child state"))
    }
}

fn assert_session_store_lock_blocked(stream: &mut UnixStream) {
    assert!(
        stream
            .set_read_timeout(Some(Duration::from_millis(200)))
            .is_ok()
    );
    let mut byte = [0_u8; 1];
    let result = stream.read_exact(&mut byte);
    assert!(result.is_err_and(|error| matches!(
        error.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    )));
    assert!(
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .is_ok()
    );
}

fn finish_session_store_lock_child(
    mut child: std::process::Child,
    stream: &mut UnixStream,
) -> std::io::Result<()> {
    expect_session_store_lock_byte(stream, b'D')?;
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("lock child failed"))
    }
}

#[test]
fn session_store_lock_child() {
    let Ok(mode) = std::env::var("CORTEXFS_LOCK_CHILD_MODE") else {
        return;
    };
    let session = PathBuf::from(ok!(std::env::var("CORTEXFS_LOCK_CHILD_SESSION")));
    let socket = PathBuf::from(ok!(std::env::var("CORTEXFS_LOCK_CHILD_SOCKET")));
    let mut stream = ok!(UnixStream::connect(socket));
    assert!(stream.write_all(b"S").is_ok());
    let result = match mode.as_str() {
        "hold-exclusive" | "hold-shared" => {
            super::columnar::with_test_store_lock(&session, mode == "hold-exclusive", || {
                stream.write_all(b"R")?;
                let mut release = [0_u8; 1];
                stream.read_exact(&mut release)?;
                Ok(())
            })
        }
        "append" => super::columnar::append(
            &session,
            super::columnar::Stream::Messages,
            &[r#"{"child":true}"#],
        ),
        "flush" => super::columnar::flush(&session),
        _ => Err(std::io::Error::other("invalid lock child mode")),
    };
    assert!(result.is_ok(), "{result:?}");
    assert!(stream.write_all(b"D").is_ok());
}

#[test]
fn session_store_flock_coordinates_real_processes() {
    let root = reference_tree("session-store-process-lock");
    let session_root = root.join("home/1000/agent/coder/session");
    ok!(ensure_durable_session_layout(
        &session_root,
        "default",
        "/work",
        Some("main"),
        SocketSessionScope::Private,
    ));
    let session = session_root.join("default");

    let (exclusive_child, mut exclusive) = ok!(spawn_session_store_lock_child(
        &session,
        &root.join("exclusive.sock"),
        "hold-exclusive",
    ));
    ok!(expect_session_store_lock_byte(&mut exclusive, b'S'));
    ok!(expect_session_store_lock_byte(&mut exclusive, b'R'));
    let (append_child, mut append) = ok!(spawn_session_store_lock_child(
        &session,
        &root.join("append.sock"),
        "append",
    ));
    ok!(expect_session_store_lock_byte(&mut append, b'S'));
    assert_session_store_lock_blocked(&mut append);
    let (flush_child, mut flush) = ok!(spawn_session_store_lock_child(
        &session,
        &root.join("flush.sock"),
        "flush",
    ));
    ok!(expect_session_store_lock_byte(&mut flush, b'S'));
    assert_session_store_lock_blocked(&mut flush);
    assert!(exclusive.write_all(b"X").is_ok());
    ok!(finish_session_store_lock_child(
        exclusive_child,
        &mut exclusive,
    ));
    ok!(finish_session_store_lock_child(append_child, &mut append));
    ok!(finish_session_store_lock_child(flush_child, &mut flush));

    let (shared_one_child, mut shared_one) = ok!(spawn_session_store_lock_child(
        &session,
        &root.join("shared-one.sock"),
        "hold-shared",
    ));
    ok!(expect_session_store_lock_byte(&mut shared_one, b'S'));
    ok!(expect_session_store_lock_byte(&mut shared_one, b'R'));
    let (shared_two_child, mut shared_two) = ok!(spawn_session_store_lock_child(
        &session,
        &root.join("shared-two.sock"),
        "hold-shared",
    ));
    ok!(expect_session_store_lock_byte(&mut shared_two, b'S'));
    ok!(expect_session_store_lock_byte(&mut shared_two, b'R'));
    assert!(shared_one.write_all(b"X").is_ok());
    assert!(shared_two.write_all(b"X").is_ok());
    ok!(finish_session_store_lock_child(
        shared_one_child,
        &mut shared_one,
    ));
    ok!(finish_session_store_lock_child(
        shared_two_child,
        &mut shared_two,
    ));

    let (shared_child, mut shared) = ok!(spawn_session_store_lock_child(
        &session,
        &root.join("shared.sock"),
        "hold-shared",
    ));
    ok!(expect_session_store_lock_byte(&mut shared, b'S'));
    ok!(expect_session_store_lock_byte(&mut shared, b'R'));
    let (waiting_child, mut waiting) = ok!(spawn_session_store_lock_child(
        &session,
        &root.join("waiting-exclusive.sock"),
        "hold-exclusive",
    ));
    ok!(expect_session_store_lock_byte(&mut waiting, b'S'));
    assert_session_store_lock_blocked(&mut waiting);
    assert!(shared.write_all(b"X").is_ok());
    ok!(finish_session_store_lock_child(shared_child, &mut shared));
    ok!(expect_session_store_lock_byte(&mut waiting, b'R'));
    assert!(waiting.write_all(b"X").is_ok());
    ok!(finish_session_store_lock_child(waiting_child, &mut waiting,));

    let actual = ok!(super::columnar::read_at(
        &session,
        super::columnar::Stream::Messages,
        0,
        1024,
    ));
    assert_eq!(actual, b"{\"child\":true}\n");
}
use super::*;
