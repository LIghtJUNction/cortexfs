use super::*;
use serde_json::Value;
use std::io::{BufRead, BufReader};

fn field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str()
}

fn parse_jsonl(text: &str) -> serde_json::Result<Vec<Value>> {
    text.lines().map(serde_json::from_str).collect()
}

fn done_statuses(frames: &[Value], run: &str) -> Result<Vec<String>, &'static str> {
    frames
        .iter()
        .filter(|value| field(value, "type") == Some("done") && field(value, "run") == Some(run))
        .map(|value| {
            field(value, "status")
                .map(str::to_owned)
                .ok_or("done missing status")
        })
        .collect()
}

#[test]
fn cancel_after_partial_delta_persists_only_cancelled_done() {
    let root = reference_tree("agent-partial-delta-cancel");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let executable = root.join("agent/coder");
    write_text_file(
        &executable,
        r#"#!/bin/sh
trap 'exit 0' TERM
printf '{"type":"delta","run":"%s","text":"partial"}\n' "$CTX_RUN_ID"
touch "$CTX_SOURCE/cancel-ready"
sleep 10
printf '{"type":"done","run":"%s","status":"ok"}\n' "$CTX_RUN_ID"
"#,
    );
    set_file_mode(&executable, 0o755);
    let (mut client, mut socket) = ok!(UnixStream::pair());
    let request = b"{\"op\":\"send\",\"id\":\"cancel-partial-1\",\"session\":\"default\",\"input\":\"run\"}\n";
    assert!(client.write_all(request).is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());
    let cancel_root = session_root.clone();
    let ready = root.join("cancel-ready");
    let cancel = thread::spawn(move || {
        let mut reader = BufReader::new(client);
        let mut response = String::new();
        loop {
            let mut line = String::new();
            let read = reader.read_line(&mut line).ok()?;
            if read == 0 {
                return None;
            }
            response.push_str(&line);
            let frame = serde_json::from_str::<Value>(&line).ok()?;
            if field(&frame, "type") == Some("delta") {
                for _ in 0..50 {
                    if ready.exists() {
                        let cancelled = handle_socket_request_frame(
                            &cancel_root,
                            "/work",
                            Some("debug/echo"),
                            r#"{"op":"cancel","id":"cancel-partial-1"}"#,
                        )
                        .is_ok();
                        return Some((cancelled, response, reader.into_inner()));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                return None;
            }
        }
    });
    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        direct_agent_runtime(&root, &view, &session_root, &executable),
    );
    let joined = cancel.join();
    assert!(matches!(&joined, Ok(Some((true, _, _)))));
    let Ok(Some((_cancelled, mut response, mut client))) = joined else {
        return;
    };
    let outcome = ok!(outcome);
    let run = ok!(response_run(&outcome));
    drop(socket);
    assert!(client.read_to_string(&mut response).is_ok());
    let session = session_root.join("default");
    let events = ok!(crate::support::columnar::read_text(
        &session,
        crate::support::columnar::Stream::Events,
        1024 * 1024,
    ));
    let statuses = ok!(done_statuses(&ok!(parse_jsonl(&events)), &run));
    let response_frames = ok!(parse_jsonl(&response));
    let client_done = ok!(done_statuses(&response_frames, &run));
    let partial = response_frames
        .iter()
        .any(|frame| field(frame, "text") == Some("partial"));
    assert_eq!(
        (
            statuses,
            ok!(fs::read_to_string(session.join("state"))),
            client_done.iter().any(|status| status == "ok"),
            partial,
        ),
        (
            vec!["cancelled".to_owned()],
            "cancelled\n".to_owned(),
            false,
            true
        )
    );
}
