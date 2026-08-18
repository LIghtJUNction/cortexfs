#![expect(
    clippy::expect_used,
    reason = "terminal fixture setup should fail loudly with its operation"
)]

use super::*;

#[test]
fn terminal_id_is_stable_for_agent_and_session() {
    assert_eq!(terminal_id("coder", "default"), "terminal-coder-default");
}

#[test]
fn ensure_layout_writes_metadata_and_event_stream() {
    let root = tempfile::tempdir().expect("temporary terminal root");
    let session = root.path().join("session");
    let record = TerminalRecord {
        id: "terminal-coder-default".to_owned(),
        agent: "coder".to_owned(),
        session: "default".to_owned(),
        owner: "1000".to_owned(),
        cwd: "/workspace".to_owned(),
        command: vec!["/ctx/bin/tsh".to_owned()],
        state: "created".to_owned(),
        socket: None,
        created_at: 1,
    };
    let directory = ensure_layout(&session, &record).expect("terminal layout");
    assert_eq!(read_record(&directory).expect("terminal metadata"), record);
    assert!(directory.join("events.jsonl").is_file());
    assert!(directory.join("status").is_file());
}

#[test]
fn append_event_preserves_non_utf8_output_as_base64() {
    let root = tempfile::tempdir().expect("temporary terminal root");
    let path = root.path().join("events.jsonl");
    append_event(&path, &TerminalEvent::output(1, 2, &[0, 255])).expect("append terminal event");
    let content = std::fs::read_to_string(path).expect("read event");
    assert!(content.contains("\"type\":\"pty.output\""));
    assert!(content.contains("\"data_b64\":\"AP8=\""));
}

#[test]
fn next_sequence_continues_existing_event_history() {
    let root = tempfile::tempdir().expect("temporary terminal root");
    let path = root.path().join("events.jsonl");
    append_event(&path, &TerminalEvent::exit(7, 8, 0)).expect("append terminal event");
    assert_eq!(next_sequence(&path).expect("read terminal sequence"), 8);
}

#[test]
fn mark_state_updates_metadata_and_text_projections() {
    let root = tempfile::tempdir().expect("temporary terminal root");
    let record = TerminalRecord {
        id: "terminal-coder-default".to_owned(),
        agent: "coder".to_owned(),
        session: "default".to_owned(),
        owner: "1000".to_owned(),
        cwd: "/workspace".to_owned(),
        command: vec!["/ctx/bin/tsh".to_owned()],
        state: "running".to_owned(),
        socket: None,
        created_at: 1,
    };
    let directory = ensure_layout(root.path(), &record).expect("terminal layout");
    mark_state(&directory.join("events.jsonl"), "exited").expect("terminal state");
    assert_eq!(
        read_record(&directory).expect("terminal metadata").state,
        "exited"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("status")).expect("terminal status"),
        "exited\n"
    );
}
