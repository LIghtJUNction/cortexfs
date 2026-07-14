#[test]
fn socket_session_recorder_rejects_symlink_required_files() {
    let root = clean_test_dir("socket-session-required-file-symlink");
    let session = root.join("default");
    let outside = clean_test_dir("socket-session-required-file-symlink-outside");
    create_complete_session_layout(&session);
    write_text_file(&outside.join("messages.jsonl"), "outside\n");
    assert!(fs::remove_file(session.join("messages.jsonl")).is_ok());
    assert!(
        symlink(
            outside.join("messages.jsonl"),
            session.join("messages.jsonl")
        )
        .is_ok()
    );

    let request = parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work/project","input":"hello"}"#,
    );
    let request = ok!(request);

    assert_eq!(
        record_unindexed_socket_request_for_test(&session, &request),
        Err(SocketSessionRecordError::MissingSessionFile(
            "messages.jsonl"
        ))
    );
    assert_file_text(&outside.join("messages.jsonl"), "outside\n");
}

#[test]
fn socket_session_recorder_cancels_without_deleting_history() {
    let root = clean_test_dir("socket-session-cancel");
    let session = root.join("default");

    create_complete_session_layout(&session);
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"keep me\"}\n",
    );
    write_text_file(
        &session.join("events.jsonl"),
        "{\"type\":\"start\",\"id\":\"run-1\",\"run\":\"run-1\",\"client_id\":\"run-1\"}\n",
    );
    write_text_file(&session.join("state"), "active\n");
    write_text_file(&session.join("current_run"), "run-1\n");

    let request = parse_socket_request_frame(r#"{"op":"cancel","id":"run-1"}"#);
    let request = ok!(request);
    let recorded = record_unindexed_socket_request_for_test(&session, &request);
    let recorded = ok!(recorded);
    assert!(recorded.messages().is_empty());
    assert_eq!(recorded.events().len(), 1);

    let events = fs::read_to_string(session.join("events.jsonl"));
    let events = ok!(events);
    assert_file_text(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"keep me\"}\n",
    );
    assert!(inspect_event_stream_jsonl(&events).is_ok());
    assert!(events.contains("\"status\":\"cancelled\""));
    assert_file_text(&session.join("state"), "cancelled\n");
}

#[test]
fn assistant_response_recorder_updates_latest_without_replacing_history() {
    let root = clean_test_dir("assistant-response-record");
    let session = root.join("default");

    create_complete_session_layout(&session);
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"hello\"}\n",
    );
    write_text_file(
        &session.join("events.jsonl"),
        "{\"type\":\"start\",\"id\":\"run-1\",\"run\":\"run-1\"}\n",
    );
    write_text_file(&session.join("latest.md"), "old\n");

    let recorded = record_assistant_response_to_session(&session, "run-1", "hello back");
    let recorded = ok!(recorded);
    assert_eq!(recorded.messages().len(), 1);
    assert_eq!(recorded.events().len(), 2);

    let messages = fs::read_to_string(session.join("messages.jsonl"));
    let messages = ok!(messages);
    let events = fs::read_to_string(session.join("events.jsonl"));
    let events = ok!(events);
    assert!(inspect_message_stream_jsonl(&messages).is_ok());
    assert!(inspect_event_stream_jsonl(&events).is_ok());
    assert!(messages.contains("\"role\":\"user\""));
    assert!(messages.contains("\"role\":\"assistant\""));
    assert!(recorded.messages().first().is_some_and(|message| {
        serde_json::from_str::<serde_json::Value>(message)
            .ok()
            .and_then(|value| {
                value
                    .get("run")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .as_deref()
            == Some("run-1")
    }));
    assert!(events.contains("\"type\":\"message\""));
    assert!(events.contains("\"status\":\"ok\""));
    assert_file_text(&session.join("latest.md"), "hello back\n");
    assert_file_text(&session.join("state"), "done\n");
}

#[test]
fn assistant_response_recorder_rejects_nul_content_without_recording() {
    let root = clean_test_dir("assistant-response-record-nul");
    let session = root.join("default");

    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(&session.join("latest.md"), "old\n");

    assert_eq!(
        record_assistant_response_to_session(&session, "run-1", "bad\0content"),
        Err(SocketSessionRecordError::InvalidField("content"))
    );
    assert_file_text(&session.join("messages.jsonl"), "");
    assert_file_text(&session.join("events.jsonl"), "");
    assert_file_text(&session.join("latest.md"), "old\n");
}

#[test]
fn agent_terminal_frames_end_only_the_matching_active_run() {
    let root = clean_test_dir("agent-terminal-session-state");
    let session = root.join("default");
    create_complete_session_layout(&session);
    write_text_file(&session.join("state"), "active\n");
    write_text_file(&session.join("current_run"), "run-ok\n");
    write_text_file(&session.join("events.jsonl"), "raw-history\n");

    let pending = vec![r#"{"type":"delta","run":"run-ok","text":"x"}"#.to_owned()];
    assert_eq!(
        crate::runtime::socket::events::record_agent_terminal_state_from_event_frames(
            &session, "run-ok", &pending,
        ),
        Ok(false)
    );
    assert_file_text(&session.join("state"), "active\n");
    assert_file_text(&session.join("current_run"), "run-ok\n");

    let done = vec![r#"{"type":"done","run":"run-ok","status":"ok"}"#.to_owned()];
    assert_eq!(
        crate::runtime::socket::events::record_agent_terminal_state_from_event_frames(
            &session, "run-ok", &done,
        ),
        Ok(true)
    );
    assert_file_text(&session.join("state"), "done\n");
    assert_file_text(&session.join("current_run"), "run-ok\n");

    let late_error = vec![r#"{"type":"done","run":"run-ok","status":"error"}"#.to_owned()];
    assert_eq!(
        crate::runtime::socket::events::record_agent_terminal_state_from_event_frames(
            &session,
            "run-ok",
            &late_error,
        ),
        Ok(false)
    );
    assert_file_text(&session.join("state"), "done\n");

    write_text_file(&session.join("state"), "active\n");
    write_text_file(&session.join("current_run"), "run-new\n");
    assert_eq!(
        crate::runtime::socket::events::record_agent_terminal_state_from_event_frames(
            &session, "run-ok", &done,
        ),
        Ok(false)
    );
    assert_file_text(&session.join("state"), "active\n");
    assert_file_text(&session.join("current_run"), "run-new\n");

    write_text_file(&session.join("current_run"), "run-blocked\n");
    let cancelled_latest = vec![
        r#"{"type":"done","run":"run-blocked","status":"ok"}"#.to_owned(),
        r#"{"type":"done","run":"run-blocked","status":"cancelled"}"#.to_owned(),
    ];
    assert_eq!(
        crate::runtime::socket::events::record_agent_terminal_state_from_event_frames(
            &session,
            "run-blocked",
            &cancelled_latest,
        ),
        Ok(false)
    );
    assert_file_text(&session.join("state"), "active\n");
    let unknown_latest = vec![
        r#"{"type":"done","run":"run-blocked","status":"ok"}"#.to_owned(),
        r#"{"type":"done","run":"run-blocked","status":"future"}"#.to_owned(),
    ];
    assert_eq!(
        crate::runtime::socket::events::record_agent_terminal_state_from_event_frames(
            &session,
            "run-blocked",
            &unknown_latest,
        ),
        Ok(false)
    );
    assert_file_text(&session.join("state"), "active\n");

    write_text_file(&session.join("state"), "active\n");
    write_text_file(&session.join("current_run"), "run-error\n");
    let error = vec![r#"{"type":"done","run":"run-error","status":"error"}"#.to_owned()];
    assert_eq!(
        crate::runtime::socket::events::record_agent_terminal_state_from_event_frames(
            &session,
            "run-error",
            &error,
        ),
        Ok(true)
    );
    assert_file_text(&session.join("state"), "error\n");
    assert_file_text(&session.join("current_run"), "run-error\n");
    let late_ok = vec![r#"{"type":"done","run":"run-error","status":"ok"}"#.to_owned()];
    assert_eq!(
        crate::runtime::socket::events::record_agent_terminal_state_from_event_frames(
            &session,
            "run-error",
            &late_ok,
        ),
        Ok(false)
    );
    assert_file_text(&session.join("state"), "error\n");
    write_text_file(&session.join("state"), "cancelled\n");
    assert_eq!(
        crate::runtime::socket::events::record_agent_terminal_state_from_event_frames(
            &session,
            "run-error",
            &late_ok,
        ),
        Ok(false)
    );
    assert_file_text(&session.join("state"), "cancelled\n");
    assert_file_text(&session.join("events.jsonl"), "raw-history\n");
}
use super::*;
use crate::runtime::record::session::set_session_state;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

#[test]
fn socket_record_replacements_preserve_session_file_metadata() {
    let root = clean_test_dir("socket-record-preserves-metadata");
    let session = root.join("default");
    create_complete_session_layout(&session);
    for file in ["state", "updated_at", "latest.md"] {
        assert!(fs::set_permissions(session.join(file), fs::Permissions::from_mode(0o640)).is_ok());
    }
    let before = ["state", "updated_at", "latest.md"].map(|file| {
        fs::metadata(session.join(file))
            .map(|metadata| {
                (
                    metadata.uid(),
                    metadata.gid(),
                    metadata.permissions().mode() & 0o777,
                )
            })
            .ok()
    });

    assert!(set_session_state(&session, "running").is_ok());
    assert!(record_assistant_response_to_session(&session, "run-1", "done").is_ok());

    let after = ["state", "updated_at", "latest.md"].map(|file| {
        fs::metadata(session.join(file))
            .map(|metadata| {
                (
                    metadata.uid(),
                    metadata.gid(),
                    metadata.permissions().mode() & 0o777,
                )
            })
            .ok()
    });
    assert_eq!(after, before);
}
