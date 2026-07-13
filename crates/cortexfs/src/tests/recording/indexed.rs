#[test]
fn indexed_socket_send_records_history_and_updates_session_index() {
    let root = clean_test_dir("indexed-socket-send");
    let session_root = root.join("session");
    let session = session_root.join("default");
    let previous = session_root.join("review-1");

    create_complete_session_layout(&session);
    create_complete_session_layout(&previous);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(
        &session_root.join("index").join("list"),
        "review-1\ndefault\n",
    );
    write_text_file(&session_root.join("index").join("current"), "review-1\n");
    assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());

    let request = parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work/project","input":"hello"}"#,
    );
    let request = ok!(request);
    let recorded = record_indexed_socket_send_to_session(&session_root, &request);
    let recorded = ok!(recorded);
    assert!(matches!(recorded, SocketSendOutcome::Recorded(_)));
    let SocketSendOutcome::Recorded(recorded) = recorded else {
        return;
    };
    assert_eq!(recorded.messages().len(), 1);
    assert_eq!(recorded.events().len(), 1);

    let by_cwd_key = session_index_key_for_cwd("/work/project");
    assert!(by_cwd_key.is_some());
    let Some(by_cwd_key) = by_cwd_key else { return };
    let messages = fs::read_to_string(session.join("messages.jsonl"));
    let messages = ok!(messages);
    let events = fs::read_to_string(session.join("events.jsonl"));
    let events = ok!(events);

    assert!(messages.contains("\"role\":\"user\""));
    assert!(messages.contains("\"run\":\"ctx-"));
    assert!(events.contains("\"type\":\"start\""));
    assert!(events.contains("\"client_id\":\"msg-1\""));
    assert!(events.contains("\"scope\":\"private\""));
    assert!(events.contains("\"cwd\":\"/work/project\""));
    assert_file_text(
        &session_root.join("index").join("list"),
        "default\nreview-1\n",
    );
    assert_file_text(&session_root.join("index").join("current"), "default\n");
    assert_file_text(
        &session_root.join("index").join("by-cwd").join(by_cwd_key),
        "default\n",
    );
}

#[test]
fn indexed_socket_send_replays_without_changing_history_or_index() {
    let root = clean_test_dir("indexed-socket-replay");
    let session_root = root.join("session");
    let session = session_root.join("default");

    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(&session_root.join("index/list"), "default\n");
    write_text_file(&session_root.join("index/current"), "default\n");
    assert!(fs::create_dir_all(session_root.join("index/by-cwd")).is_ok());
    let request = ok!(parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work/project","input":"hello"}"#,
    ));
    assert!(matches!(
        record_indexed_socket_send_to_session(&session_root, &request),
        Ok(SocketSendOutcome::Recorded(_))
    ));
    let by_cwd = session_index_key_for_cwd("/work/project");
    assert!(by_cwd.is_some(), "stable cwd should have an index key");
    let Some(by_cwd) = by_cwd else {
        return;
    };
    let paths = [
        session.join("messages.jsonl"),
        session.join("events.jsonl"),
        session_root.join("index/list"),
        session_root.join("index/current"),
        session_root.join("index/by-cwd").join(by_cwd),
    ];
    let before = paths.iter().map(fs::read).collect::<Result<Vec<_>, _>>();
    let before = ok!(before);

    let replayed = record_indexed_socket_send_to_session(&session_root, &request);
    assert!(matches!(
        replayed,
        Ok(SocketSendOutcome::Replayed(ref record))
            if record.messages().is_empty()
                && record.events().len() == 1
                && record
                    .events()
                    .first()
                    .is_some_and(|event| event.contains("\"type\":\"start\""))
    ));
    let after = paths.iter().map(fs::read).collect::<Result<Vec<_>, _>>();
    let after = ok!(after);
    assert_eq!(after, before);
}

#[test]
fn indexed_socket_send_maps_client_id_to_canonical_durable_run_for_replay() {
    let root = clean_test_dir("indexed-socket-client-run-map");
    let session_root = root.join("session");
    let session = session_root.join("default");

    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(&session_root.join("index/list"), "default\n");
    write_text_file(&session_root.join("index/current"), "default\n");
    assert!(fs::create_dir_all(session_root.join("index/by-cwd")).is_ok());
    let request = ok!(parse_socket_request_frame(
        r#"{"op":"send","id":"client-msg-id","session":"default","cwd":"/work/project","input":"hello"}"#,
    ));

    let recorded = ok!(record_indexed_socket_send_to_session(
        &session_root,
        &request,
    ));
    assert!(
        matches!(recorded, SocketSendOutcome::Recorded(_)),
        "first send should record a durable run"
    );
    let SocketSendOutcome::Recorded(recorded) = recorded else {
        return;
    };
    assert!(
        !recorded.messages().is_empty(),
        "first send should record a user message"
    );
    let Some(message) = recorded.messages().first() else {
        return;
    };
    let message = ok!(serde_json::from_str::<serde_json::Value>(message));
    let run = message.get("run").and_then(serde_json::Value::as_str);
    assert!(run.is_some(), "user message should name its durable run");
    let Some(run) = run else { return };
    assert_ne!(run, "client-msg-id");
    assert_eq!(run.len(), 36);
    let suffix = run.strip_prefix("ctx-");
    assert!(suffix.is_some(), "durable run should use the ctx- prefix");
    let Some(suffix) = suffix else { return };
    assert!(
        suffix.len() == 32
            && suffix
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    );
    let first_run = run.to_owned();
    let before_messages = ok!(fs::read(session.join("messages.jsonl")));
    let before_events = ok!(fs::read(session.join("events.jsonl")));

    let replayed = ok!(record_indexed_socket_send_to_session(
        &session_root,
        &request,
    ));
    assert!(
        matches!(replayed, SocketSendOutcome::Replayed(_)),
        "same client id and payload should replay"
    );
    let SocketSendOutcome::Replayed(replayed) = replayed else {
        return;
    };
    assert!(
        !replayed.events().is_empty(),
        "replay should return the original start frame"
    );
    let Some(frame) = replayed.events().first() else {
        return;
    };
    let frame = ok!(serde_json::from_str::<serde_json::Value>(frame));
    assert_eq!(
        frame.get("id").and_then(serde_json::Value::as_str),
        Some(first_run.as_str())
    );
    assert_eq!(
        frame.get("run").and_then(serde_json::Value::as_str),
        Some(first_run.as_str())
    );
    assert_eq!(
        frame.get("client_id").and_then(serde_json::Value::as_str),
        Some("client-msg-id")
    );
    assert_eq!(
        ok!(fs::read(session.join("messages.jsonl"))),
        before_messages
    );
    assert_eq!(ok!(fs::read(session.join("events.jsonl"))), before_events);
}

#[test]
fn indexed_socket_send_replay_repairs_index_after_post_commit_failure() {
    let root = clean_test_dir("indexed-socket-replay-index-repair");
    let session_root = root.join("session");
    let session = session_root.join("default");

    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(&session_root.join("index/list"), "review-1\ndefault\n");
    write_text_file(&session_root.join("index/current"), "review-1\n");
    assert!(fs::create_dir_all(session_root.join("index/by-cwd")).is_ok());
    let request = ok!(parse_socket_request_frame(
        r#"{"op":"send","id":"client-msg-id","session":"default","cwd":"/work/project","input":"hello"}"#,
    ));
    let by_cwd = session_index_key_for_cwd("/work/project");
    assert!(by_cwd.is_some(), "stable cwd should have an index key");
    let Some(by_cwd) = by_cwd else { return };

    crate::support::index::set_session_index_update_failure(true);
    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &request),
        Err(IndexedSocketSessionRecordError::Index(
            SessionIndexUpdateError::CannotRecord
        ))
    );
    let before_messages = ok!(fs::read(session.join("messages.jsonl")));
    let before_events = ok!(fs::read(session.join("events.jsonl")));
    assert_eq!(String::from_utf8_lossy(&before_messages).lines().count(), 1);
    assert_eq!(String::from_utf8_lossy(&before_events).lines().count(), 1);
    assert_file_text(&session_root.join("index/list"), "review-1\ndefault\n");
    assert_file_text(&session_root.join("index/current"), "review-1\n");
    assert!(!session_root.join("index/by-cwd").join(&by_cwd).exists());

    assert!(matches!(
        record_indexed_socket_send_to_session(&session_root, &request),
        Ok(SocketSendOutcome::Replayed(_))
    ));
    assert_eq!(
        ok!(fs::read(session.join("messages.jsonl"))),
        before_messages
    );
    assert_eq!(ok!(fs::read(session.join("events.jsonl"))), before_events);
    assert_file_text(&session_root.join("index/list"), "default\nreview-1\n");
    assert_file_text(&session_root.join("index/current"), "default\n");
    assert_file_text(&session_root.join("index/by-cwd").join(by_cwd), "default\n");
}

fn assert_indexed_socket_send_pair_recovers_after_start_failure(columnar_store: bool) {
    let backend = if columnar_store { "columnar" } else { "legacy" };
    let root = clean_test_dir(&format!("indexed-socket-send-pair-{backend}"));
    let session_root = root.join("session");
    let session = session_root.join("default");

    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(&session_root.join("index/list"), "default\n");
    write_text_file(&session_root.join("index/current"), "default\n");
    assert!(fs::create_dir_all(session_root.join("index/by-cwd")).is_ok());
    let baseline = if columnar_store {
        assert!(
            crate::support::columnar::append(
                &session,
                crate::support::columnar::Stream::Messages,
                &[r#"{"role":"user","run":"seed","content":"seed"}"#],
            )
            .is_ok()
        );
        assert!(
            crate::support::columnar::append(
                &session,
                crate::support::columnar::Stream::Events,
                &[r#"{"type":"start","id":"seed","run":"seed","scope":"private","cwd":"/seed"}"#],
            )
            .is_ok()
        );
        1
    } else {
        0
    };
    let guard = ok!(crate::support::columnar::HistoryGuard::exclusive(&session));
    assert_eq!(guard.is_columnar(), columnar_store);
    drop(guard);
    let request = ok!(parse_socket_request_frame(
        r#"{"op":"send","id":"client-msg-id","session":"default","cwd":"/work/project","input":"hello"}"#,
    ));

    crate::support::columnar::set_send_pair_event_failure(true);
    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &request),
        Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::CannotRecord
        ))
    );
    let messages = ok!(crate::support::columnar::read_text(
        &session,
        crate::support::columnar::Stream::Messages,
        64 * 1024,
    ));
    let events = ok!(crate::support::columnar::read_text(
        &session,
        crate::support::columnar::Stream::Events,
        64 * 1024,
    ));
    assert_eq!(messages.lines().count(), baseline + 1, "{messages}");
    assert_eq!(events.lines().count(), baseline, "{events}");
    let message = ok!(serde_json::from_str::<serde_json::Value>(
        messages.lines().last().unwrap_or_default()
    ));
    let run = message
        .get("run")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    assert!(
        run.is_some(),
        "prepared user message should name its durable run"
    );
    let Some(run) = run else { return };
    let receipt_path = session.join(".store/send/client-msg-id");
    let receipt_metadata = ok!(fs::symlink_metadata(&receipt_path));
    assert!(receipt_metadata.is_file());
    assert!(!receipt_metadata.file_type().is_symlink());
    assert_eq!(receipt_metadata.permissions().mode() & 0o7777, 0o600);
    let receipt = ok!(fs::read_to_string(&receipt_path));
    let receipt = ok!(serde_json::from_str::<serde_json::Value>(&receipt));
    assert_eq!(
        receipt.get("version").and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        receipt.get("id").and_then(serde_json::Value::as_str),
        Some("client-msg-id")
    );
    assert_eq!(
        receipt.get("run").and_then(serde_json::Value::as_str),
        Some(run.as_str())
    );
    let interleaved = ok!(parse_socket_request_frame(
        r#"{"op":"send","id":"other-msg-id","session":"default","cwd":"/work/project","input":"other"}"#,
    ));
    assert!(matches!(
        record_indexed_socket_send_to_session(&session_root, &interleaved),
        Ok(SocketSendOutcome::Recorded(_))
    ));
    assert!(receipt_path.exists());

    assert!(matches!(
        record_indexed_socket_send_to_session(&session_root, &request),
        Ok(SocketSendOutcome::Recorded(_))
    ));
    let messages = ok!(crate::support::columnar::read_text(
        &session,
        crate::support::columnar::Stream::Messages,
        64 * 1024,
    ));
    let events = ok!(crate::support::columnar::read_text(
        &session,
        crate::support::columnar::Stream::Events,
        64 * 1024,
    ));
    assert_eq!(messages.lines().count(), baseline + 2, "{messages}");
    assert_eq!(events.lines().count(), baseline + 2, "{events}");
    let event = ok!(serde_json::from_str::<serde_json::Value>(
        events.lines().last().unwrap_or_default()
    ));
    assert_eq!(
        event.get("run").and_then(serde_json::Value::as_str),
        Some(run.as_str())
    );
    assert!(!receipt_path.exists());
}

#[test]
fn indexed_socket_send_pair_recovers_legacy_after_start_failure() {
    assert_indexed_socket_send_pair_recovers_after_start_failure(false);
}

#[test]
fn indexed_socket_send_pair_recovers_columnar_after_start_failure() {
    assert_indexed_socket_send_pair_recovers_after_start_failure(true);
}

#[test]
fn indexed_socket_send_pair_recovers_legacy_after_large_interleaved_growth() {
    let root = clean_test_dir("indexed-socket-send-pair-legacy-large-interleave");
    let session_root = root.join("session");
    let session = session_root.join("default");
    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(&session_root.join("index/list"), "default\n");
    write_text_file(&session_root.join("index/current"), "default\n");
    assert!(fs::create_dir_all(session_root.join("index/by-cwd")).is_ok());
    let request = ok!(parse_socket_request_frame(
        r#"{"op":"send","id":"client-a","session":"default","cwd":"/work","input":"alpha"}"#,
    ));

    crate::support::columnar::set_send_pair_event_failure(true);
    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &request),
        Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::CannotRecord
        ))
    );
    let messages = ok!(fs::read_to_string(session.join("messages.jsonl")));
    let first = ok!(serde_json::from_str::<serde_json::Value>(
        messages.lines().next().unwrap_or_default()
    ));
    let Some(run) = first
        .get("run")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
    else {
        assert!(
            first
                .get("run")
                .and_then(serde_json::Value::as_str)
                .is_some(),
            "prepared user message should retain its durable run"
        );
        return;
    };

    for (id, fill) in [("client-b", 'b'), ("client-c", 'c')] {
        let input = fill.to_string().repeat(140 * 1024);
        let frame = serde_json::json!({
            "op": "send",
            "id": id,
            "session": "default",
            "cwd": "/work",
            "input": input,
        })
        .to_string();
        let interleaved = ok!(parse_socket_request_frame(&frame));
        assert!(matches!(
            record_indexed_socket_send_to_session(&session_root, &interleaved),
            Ok(SocketSendOutcome::Recorded(_))
        ));
    }

    assert!(matches!(
        record_indexed_socket_send_to_session(&session_root, &request),
        Ok(SocketSendOutcome::Recorded(_))
    ));
    let messages = ok!(fs::read_to_string(session.join("messages.jsonl")));
    let events = ok!(fs::read_to_string(session.join("events.jsonl")));
    assert_eq!(
        messages
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|value| {
                value.get("run").and_then(serde_json::Value::as_str) == Some(run.as_str())
            })
            .count(),
        1,
        "{messages}"
    );
    assert_eq!(
        events
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|value| {
                value.get("type").and_then(serde_json::Value::as_str) == Some("start")
                    && value.get("run").and_then(serde_json::Value::as_str) == Some(run.as_str())
            })
            .count(),
        1,
        "{events}"
    );
}

#[test]
fn indexed_socket_send_pair_rejects_unexpected_fact_for_pending_run() {
    let root = clean_test_dir("indexed-socket-send-pair-exact-fact");
    let session_root = root.join("session");
    let session = session_root.join("default");
    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(&session_root.join("index/list"), "default\n");
    write_text_file(&session_root.join("index/current"), "default\n");
    assert!(fs::create_dir_all(session_root.join("index/by-cwd")).is_ok());
    let request = ok!(parse_socket_request_frame(
        r#"{"op":"send","id":"client-msg-id","session":"default","cwd":"/work","input":"hello"}"#,
    ));
    crate::support::columnar::set_send_pair_event_failure(true);
    assert!(record_indexed_socket_send_to_session(&session_root, &request).is_err());
    let messages = ok!(fs::read_to_string(session.join("messages.jsonl")));
    let message = ok!(serde_json::from_str::<serde_json::Value>(
        messages.lines().next().unwrap_or_default()
    ));
    let run = message.get("run").and_then(serde_json::Value::as_str);
    assert!(
        run.is_some(),
        "prepared user message should name its durable run"
    );
    let Some(run) = run else { return };
    let unexpected = serde_json::json!({
        "role": "assistant",
        "run": run,
        "content": "unexpected",
    })
    .to_string();
    let history = ok!(crate::support::columnar::HistoryGuard::exclusive(&session));
    assert!(
        history
            .append(crate::support::columnar::Stream::Messages, &[&unexpected],)
            .is_ok()
    );
    drop(history);

    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &request),
        Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::CorruptHistory
        ))
    );
    assert_file_text(&session.join("events.jsonl"), "");
    assert!(session.join(".store/send/client-msg-id").exists());
}

#[test]
fn indexed_socket_send_pair_refuses_replaced_receipt_during_cleanup() {
    let root = clean_test_dir("indexed-socket-send-pair-cleanup-replace");
    let session_root = root.join("session");
    let session = session_root.join("default");
    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(&session_root.join("index/list"), "default\n");
    write_text_file(&session_root.join("index/current"), "default\n");
    assert!(fs::create_dir_all(session_root.join("index/by-cwd")).is_ok());
    let request = ok!(parse_socket_request_frame(
        r#"{"op":"send","id":"client-msg-id","session":"default","cwd":"/work","input":"hello"}"#,
    ));
    crate::support::columnar::set_send_pair_event_failure(true);
    assert!(record_indexed_socket_send_to_session(&session_root, &request).is_err());

    crate::support::columnar::set_send_receipt_replacement(true);
    let outcome = record_indexed_socket_send_to_session(&session_root, &request);
    crate::support::columnar::set_send_receipt_replacement(false);
    assert_eq!(
        outcome,
        Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::CannotRecord
        ))
    );
    assert_file_text(&session.join(".store/send/client-msg-id"), "replacement\n");
    assert!(session.join(".store/send/.client-msg-id.captured").exists());
}

#[test]
fn indexed_socket_send_claims_one_id_once_across_concurrent_runtimes() {
    let root = clean_test_dir("indexed-socket-concurrent-claim");
    let session_root = root.join("session");
    let session = session_root.join("default");
    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(&session_root.join("index/list"), "default\n");
    write_text_file(&session_root.join("index/current"), "default\n");
    assert!(fs::create_dir_all(session_root.join("index/by-cwd")).is_ok());
    let request = ok!(parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work","input":"hello"}"#,
    ));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut workers = Vec::new();
    for _worker in 0..2 {
        let barrier = std::sync::Arc::clone(&barrier);
        let session_root = session_root.clone();
        let request = request.clone();
        workers.push(thread::spawn(move || {
            barrier.wait();
            record_indexed_socket_send_to_session(&session_root, &request)
        }));
    }
    barrier.wait();
    let outcomes = workers
        .into_iter()
        .filter_map(|worker| worker.join().ok())
        .collect::<Vec<_>>();

    assert_eq!(outcomes.len(), 2);
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, Ok(SocketSendOutcome::Recorded(_))))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, Ok(SocketSendOutcome::Replayed(_))))
            .count(),
        1,
        "{outcomes:?}"
    );
    assert_eq!(
        ok!(fs::read_to_string(session.join("messages.jsonl")))
            .lines()
            .count(),
        1
    );
    assert_eq!(
        ok!(fs::read_to_string(session.join("events.jsonl")))
            .lines()
            .count(),
        1
    );
}

#[test]
fn indexed_socket_send_rejects_conflicting_payload_without_changes() {
    let root = clean_test_dir("indexed-socket-conflict");
    let session_root = root.join("session");
    let session = session_root.join("default");

    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(&session_root.join("index/list"), "default\n");
    write_text_file(&session_root.join("index/current"), "default\n");
    assert!(fs::create_dir_all(session_root.join("index/by-cwd")).is_ok());
    let original = ok!(parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work/project","input":"hello"}"#,
    ));
    assert!(matches!(
        record_indexed_socket_send_to_session(&session_root, &original),
        Ok(SocketSendOutcome::Recorded(_))
    ));
    let before_messages = ok!(fs::read(session.join("messages.jsonl")));
    let before_events = ok!(fs::read(session.join("events.jsonl")));
    let before_list = ok!(fs::read(session_root.join("index/list")));
    let before_current = ok!(fs::read(session_root.join("index/current")));
    let before_by_cwd = ok!(fs::read_dir(session_root.join("index/by-cwd")));
    let mut before_by_cwd = before_by_cwd
        .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
        .collect::<Vec<_>>();
    before_by_cwd.sort();

    for frame in [
        r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work/project","input":"different"}"#,
        r#"{"op":"send","id":"msg-1","session":"default","cwd":"/other","input":"hello"}"#,
        r#"{"op":"send","id":"msg-1","session":"default","scope":"shared","cwd":"/work/project","input":"hello"}"#,
    ] {
        let conflict = ok!(parse_socket_request_frame(frame));
        assert_eq!(
            record_indexed_socket_send_to_session(&session_root, &conflict),
            Err(IndexedSocketSessionRecordError::Session(
                SocketSessionRecordError::RequestConflict
            ))
        );
    }
    assert_eq!(
        ok!(fs::read(session.join("messages.jsonl"))),
        before_messages
    );
    assert_eq!(ok!(fs::read(session.join("events.jsonl"))), before_events);
    assert_eq!(ok!(fs::read(session_root.join("index/list"))), before_list);
    assert_eq!(
        ok!(fs::read(session_root.join("index/current"))),
        before_current
    );
    let after_by_cwd = ok!(fs::read_dir(session_root.join("index/by-cwd")));
    let mut after_by_cwd = after_by_cwd
        .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
        .collect::<Vec<_>>();
    after_by_cwd.sort();
    assert_eq!(after_by_cwd, before_by_cwd);
}

#[test]
fn indexed_socket_send_rejects_invalid_or_torn_history_without_changes() {
    for (case, corrupt_events) in [
        ("invalid", "{\n"),
        ("torn", r#"{"type":"unrelated"}"#),
        (
            "orphan-run",
            "{\"type\":\"delta\",\"run\":\"msg-1\",\"text\":\"orphan\"}\n",
        ),
    ] {
        let root = clean_test_dir(&format!("indexed-socket-corrupt-{case}"));
        let session_root = root.join("session");
        let session = session_root.join("default");
        create_complete_session_layout(&session);
        write_text_file(&session.join("messages.jsonl"), "");
        write_text_file(&session.join("events.jsonl"), corrupt_events);
        write_text_file(&session_root.join("index/list"), "default\n");
        write_text_file(&session_root.join("index/current"), "default\n");
        assert!(fs::create_dir_all(session_root.join("index/by-cwd")).is_ok());
        let request = ok!(parse_socket_request_frame(
            r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work","input":"hello"}"#,
        ));
        let before_messages = ok!(fs::read(session.join("messages.jsonl")));
        let before_events = ok!(fs::read(session.join("events.jsonl")));
        let before_list = ok!(fs::read(session_root.join("index/list")));

        assert_eq!(
            record_indexed_socket_send_to_session(&session_root, &request),
            Err(IndexedSocketSessionRecordError::Session(
                SocketSessionRecordError::CorruptHistory
            ))
        );
        assert_eq!(
            ok!(fs::read(session.join("messages.jsonl"))),
            before_messages
        );
        assert_eq!(ok!(fs::read(session.join("events.jsonl"))), before_events);
        assert_eq!(ok!(fs::read(session_root.join("index/list"))), before_list);
    }
}

#[test]
fn indexed_socket_send_rejects_orphan_message_run_fact() {
    let root = clean_test_dir("indexed-socket-orphan-message");
    let session_root = root.join("session");
    let session = session_root.join("default");
    create_complete_session_layout(&session);
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"assistant\",\"run\":\"msg-1\",\"content\":\"orphan\"}\n",
    );
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(&session_root.join("index/list"), "default\n");
    write_text_file(&session_root.join("index/current"), "default\n");
    assert!(fs::create_dir_all(session_root.join("index/by-cwd")).is_ok());
    let request = ok!(parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work","input":"hello"}"#,
    ));

    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &request),
        Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::CorruptHistory
        ))
    );
    assert_file_text(
        &session.join("messages.jsonl"),
        "{\"role\":\"assistant\",\"run\":\"msg-1\",\"content\":\"orphan\"}\n",
    );
    assert_file_text(&session.join("events.jsonl"), "");
}

#[test]
fn indexed_socket_send_rejects_noncanonical_done_status() {
    let root = clean_test_dir("indexed-socket-invalid-done-status");
    let session_root = root.join("session");
    let session = session_root.join("default");
    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(&session_root.join("index/list"), "default\n");
    write_text_file(&session_root.join("index/current"), "default\n");
    assert!(fs::create_dir_all(session_root.join("index/by-cwd")).is_ok());
    let request = ok!(parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work","input":"hello"}"#,
    ));
    assert!(matches!(
        record_indexed_socket_send_to_session(&session_root, &request),
        Ok(SocketSendOutcome::Recorded(_))
    ));
    assert!(
        append_jsonl_line(
            &session.join("events.jsonl"),
            r#"{"type":"done","run":"msg-1","status":"unknown"}"#,
        )
        .is_ok()
    );
    let before = ok!(fs::read(session.join("events.jsonl")));

    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &request),
        Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::CorruptHistory
        ))
    );
    assert_eq!(ok!(fs::read(session.join("events.jsonl"))), before);
}

#[test]
fn indexed_socket_send_preflights_index_before_recording_history() {
    let root = clean_test_dir("indexed-socket-send-bad-index");
    let session_root = root.join("session");
    let session = session_root.join("default");

    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(&session_root.join("index").join("list"), "bad/name\n");
    write_text_file(&session_root.join("index").join("current"), "default\n");
    assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());

    let request = parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work/project","input":"hello"}"#,
    );
    let request = ok!(request);

    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &request),
        Err(IndexedSocketSessionRecordError::Index(
            SessionIndexUpdateError::InvalidIndex
        ))
    );
    assert_file_text(&session.join("messages.jsonl"), "");
    assert_file_text(&session.join("events.jsonl"), "");
}

#[test]
fn indexed_socket_send_rejects_non_send_requests() {
    let root = clean_test_dir("indexed-socket-non-send");
    let session_root = root.join("session");

    let resume = parse_socket_request_frame(r#"{"op":"resume","session":"default"}"#);
    let resume = ok!(resume);
    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &resume),
        Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::UnsupportedRequest
        ))
    );

    let cancel = parse_socket_request_frame(r#"{"op":"cancel","id":"run-1"}"#);
    let cancel = ok!(cancel);
    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &cancel),
        Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::UnsupportedRequest
        ))
    );

    let ping = parse_socket_request_frame(r#"{"op":"ping"}"#);
    let ping = ok!(ping);
    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &ping),
        Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::UnsupportedRequest
        ))
    );
}
use super::*;
