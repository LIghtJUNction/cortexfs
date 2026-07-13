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
    assert!(messages.contains("\"run\":\"msg-1\""));
    assert!(events.contains("\"type\":\"start\""));
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
    let Some(by_cwd) = session_index_key_for_cwd("/work/project") else {
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
