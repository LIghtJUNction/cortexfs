use super::*;

fn session(name: &str, state: &str) -> (TestDir, PathBuf) {
    let root = clean_test_dir(name);
    let session = root.join("default");
    create_complete_session_layout(&session);
    write_text_file(&session.join("state"), &format!("{state}\n"));
    write_text_file(&session.join("current_run"), "run-1\n");
    write_text_file(
        &session.join("events.jsonl"),
        "{\"type\":\"start\",\"run\":\"run-1\"}\n",
    );
    (root, session)
}

#[test]
fn batch_filters_runs_and_ignores_approval_frames() {
    let (_root, session) = session("agent-frame-batch", "active");
    let frames = [
        r#"{"type":"tool_call","run":"other","id":"call-1","name":"bad.tool"}"#,
        r#"{"type":"message","run":"other","role":"tool","name":"bad.tool","content":[{"type":"tool_result","tool_call_id":"call-1","content":"foreign-tool"}]}"#,
        r#"{"type":"delta","run":"other","text":"foreign-assistant"}"#,
        r#"{"type":"error","run":"other","message":"foreign-error","recoverable":false}"#,
        r#"{"type":"done","run":"other","status":"error"}"#,
        r#"{"type":"message","run":"run-1","role":"tool","content":[{"type":"tool_result","tool_call_id":"call-1","content":"ok"}]}"#,
        r#"{"type":"approval_request","run":"run-1","id":"approval-1","name":"tsh","args":[]}"#,
        r#"{"type":"approval_request","run":"other","id":"foreign-approval","name":"tsh","args":[]}"#,
        "not-json",
        r#"{"type":"delta","run":"run-1","text":"answer"}"#,
        r#"{"type":"tool_call","run":"run-1","id":"call-1","name":"fs.read"}"#,
        r#"{"type":"done","run":"run-1","status":"ok"}"#,
    ]
    .map(str::to_owned);
    let batch = crate::runtime::socket::events::AgentFrameBatch::parse("run-1", &frames);
    assert_eq!(batch.settle(&session, "run-1"), Ok(true));

    let events = ok!(fs::read_to_string(session.join("events.jsonl")));
    let types = events
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|value| value.get("type")?.as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    assert_eq!(types, ["start", "message", "done"]);
    assert!(!events.contains("\"name\":\"fs.read\""));
    assert!(!events.contains("bad.tool") && !events.contains("foreign-approval"));
    assert!(inspect_event_stream_jsonl(&events).is_ok());
    let messages = ok!(fs::read_to_string(session.join("messages.jsonl")));
    assert!(!messages.contains("foreign-") && inspect_message_stream_jsonl(&messages).is_ok());
}

#[test]
fn cancelled_batch_does_not_settle() {
    let (_root, session) = session("agent-frame-cancel", "cancelled");
    let frames = [
        r#"{"type":"approval_request","run":"run-1","id":"approval-1","name":"tsh","args":[]}"#,
        r#"{"type":"message","run":"run-1","role":"tool","name":"fs.read","content":[{"type":"tool_result","tool_call_id":"call-1","content":"ok"}]}"#,
        r#"{"type":"delta","run":"run-1","text":"late"}"#,
        r#"{"type":"done","run":"run-1","status":"ok"}"#,
    ]
    .map(str::to_owned);
    let batch = crate::runtime::socket::events::AgentFrameBatch::parse("run-1", &frames);
    assert_eq!(batch.settle(&session, "run-1"), Ok(false));
    assert_file_text(&session.join("state"), "cancelled\n");
    let events = ok!(fs::read_to_string(session.join("events.jsonl")));
    assert!(!events.contains("approval-1") && !events.contains("\"name\":\"fs.read\""));
    assert!(!events.contains("\"status\":\"ok\""));
    assert!(inspect_event_stream_jsonl(&events).is_ok());
}

#[test]
fn batch_ignores_tool_and_approval_results_before_terminal_settlement() {
    let (_root, session) = session("agent-frame-error", "active");
    let frames = [
        r#"{"type":"approval_request","run":"run-1","id":"approval-1","name":"tsh","args":[]}"#,
        r#"{"type":"message","run":"run-1","role":"tool","name":"bad/tool","content":[{"type":"tool_result","tool_call_id":"call-1","content":"bad"}]}"#,
        r#"{"type":"done","run":"run-1","status":"ok"}"#,
    ]
    .map(str::to_owned);
    let batch = crate::runtime::socket::events::AgentFrameBatch::parse("run-1", &frames);
    assert_eq!(batch.settle(&session, "run-1"), Ok(true));
    assert_file_text(&session.join("state"), "done\n");
    let events = ok!(fs::read_to_string(session.join("events.jsonl")));
    assert!(!events.contains("approval-1") && !events.contains("bad/tool"));
    assert!(events.contains("\"type\":\"done\""));
    assert!(inspect_event_stream_jsonl(&events).is_ok());
}

#[test]
fn batch_uses_last_done_and_nonrecoverable_error() {
    let (_root, session) = session("agent-frame-terminal", "active");
    let frames = [
        r#"{"type":"delta","run":"run-1","text":"discarded"}"#,
        r#"{"type":"done","run":"run-1","status":"ok"}"#,
        r#"{"type":"error","run":"run-1","code":"EIO","message":"old","recoverable":false}"#,
        r#"{"type":"error","run":"run-1","code":"EIO","message":"selected","recoverable":false}"#,
        r#"{"type":"error","run":"run-1","code":"EIO","message":"recoverable","recoverable":true}"#,
        r#"{"type":"done","run":"run-1","status":"error"}"#,
    ]
    .map(str::to_owned);
    let batch = crate::runtime::socket::events::AgentFrameBatch::parse("run-1", &frames);
    assert_eq!(batch.settle(&session, "run-1"), Ok(true));

    assert_file_text(&session.join("state"), "error\n");
    let state = ok!(fs::read_to_string(session.join("state.json")));
    assert!(state.contains(r#""error":"EIO""#));
    let events = ok!(fs::read_to_string(session.join("events.jsonl")));
    assert!(events.contains("selected") && !events.contains("old"));
    assert!(
        !events.contains("\"message\":\"recoverable\"")
            && inspect_event_stream_jsonl(&events).is_ok()
    );
}
