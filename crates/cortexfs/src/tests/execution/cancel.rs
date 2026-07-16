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

fn write_cancellable_partial_agent(executable: &Path) {
    // Short cancellable wait: CI that misses cancel must not burn package timeouts.
    write_text_file(
        executable,
        r#"#!/bin/sh
trap 'exit 0' TERM INT
printf '{"type":"delta","run":"%s","text":"partial"}\n' "$CTX_RUN_ID"
touch "$CTX_SOURCE/cancel-ready"
i=0
while [ "$i" -lt 50 ]; do
  sleep 0.1
  i=$((i + 1))
done
printf '{"type":"done","run":"%s","status":"ok"}\n' "$CTX_RUN_ID"
"#,
    );
    set_file_mode(executable, 0o755);
}

fn set_stream_timeouts(stream: &UnixStream, seconds: u64) {
    let timeout = Some(Duration::from_secs(seconds));
    assert!(stream.set_read_timeout(timeout).is_ok());
    assert!(stream.set_write_timeout(timeout).is_ok());
}

fn wait_delta_then_cancel(
    client: UnixStream,
    cancel_root: &Path,
    ready: &Path,
) -> Option<(bool, String, UnixStream)> {
    let mut reader = BufReader::new(client);
    let mut response = String::new();
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return None,
            Ok(_n) => {}
        }
        response.push_str(&line);
        let frame = serde_json::from_str::<Value>(&line).ok()?;
        if field(&frame, "type") != Some("delta") {
            continue;
        }
        for _ in 0..100 {
            if ready.exists() {
                let cancelled = handle_socket_request_frame(
                    cancel_root,
                    "/work",
                    Some("debug/echo"),
                    r#"{"op":"cancel","id":"cancel-partial-1"}"#,
                )
                .is_ok();
                return Some((cancelled, response, reader.into_inner()));
            }
            thread::sleep(Duration::from_millis(20));
        }
        return None;
    }
}

#[test]
fn cancel_after_partial_delta_persists_only_cancelled_done() {
    let root = reference_tree("agent-partial-delta-cancel");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let executable = root.join("agent/coder");
    write_cancellable_partial_agent(&executable);
    let (mut client, mut socket) = ok!(UnixStream::pair());
    // Bound both sides so a missed cancel/delta cannot hang the lib suite on CI.
    set_stream_timeouts(&client, 8);
    set_stream_timeouts(&socket, 8);
    let request = b"{\"op\":\"send\",\"id\":\"cancel-partial-1\",\"session\":\"default\",\"input\":\"run\"}\n";
    assert!(client.write_all(request).is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());
    let cancel_root = session_root.clone();
    let ready = root.join("cancel-ready");
    let cancel = thread::spawn(move || wait_delta_then_cancel(client, &cancel_root, &ready));
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
    set_stream_timeouts(&client, 2);
    let mut tail = String::new();
    let _ignored = client.read_to_string(&mut tail);
    response.push_str(&tail);
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
