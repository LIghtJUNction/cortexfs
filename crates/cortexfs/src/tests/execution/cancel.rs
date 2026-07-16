use super::*;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::time::Instant;

fn write_cancellable_partial_agent(executable: &Path) {
    // Short cancellable wait so a missed cancel cannot hang the suite on CI.
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

fn wait_delta_then_cancel(
    client: UnixStream,
    cancel_root: &Path,
    ready: &Path,
) -> Option<(bool, String, UnixStream)> {
    let mut reader = BufReader::new(client);
    let mut response = String::new();
    let ready_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return None,
            Ok(_) => {}
        }
        response.push_str(&line);
        let frame: Value = serde_json::from_str(&line).ok()?;
        if json_str(&frame, "type") != Some("delta") {
            continue;
        }
        while Instant::now() < ready_deadline {
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
    set_stream_timeouts(&client, 8);
    set_stream_timeouts(&socket, 8);
    let request =
        b"{\"op\":\"send\",\"id\":\"cancel-partial-1\",\"session\":\"default\",\"input\":\"run\"}\n";
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
        .any(|frame| json_str(frame, "text") == Some("partial"));
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
