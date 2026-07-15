use super::*;
use serde_json::Value;

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
fn done_is_emitted_only_after_terminal_facts_are_durable() {
    let root = reference_tree("agent-terminal-settlement");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let executable = root.join("agent/coder");
    let session = crate::shell_single_quote(&session_root.join("default").display().to_string());
    let moved =
        crate::shell_single_quote(&session_root.join("moved-default").display().to_string());
    write_text_file(
        &executable,
        &format!(
            r#"#!/bin/sh
IFS= read -r envelope || exit 2
printf '{{"type":"delta","run":"%s","text":"settled"}}\n' "$CTX_RUN_ID"
mv -- {session} {moved}
"#
        ),
    );
    set_file_mode(&executable, 0o755);
    let (mut client, mut socket) = ok!(UnixStream::pair());
    let request =
        b"{\"op\":\"send\",\"id\":\"settlement-1\",\"session\":\"default\",\"input\":\"run\"}\n";
    assert!(client.write_all(request).is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let result = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        direct_agent_runtime(&root, &view, &session_root, &executable),
    );
    assert!(
        matches!(result, Err(SocketRuntimeError::Record(_))),
        "{result:?}"
    );
    drop(socket);
    let mut response = String::new();
    assert!(client.read_to_string(&mut response).is_ok());
    let frames = ok!(parse_jsonl(&response));
    assert!(
        frames
            .iter()
            .any(|frame| field(frame, "type") == Some("error"))
    );
    assert!(
        !frames
            .iter()
            .any(|frame| field(frame, "type") == Some("done"))
    );
}

#[test]
fn empty_success_persists_done_and_replays_without_execution() {
    let root = reference_tree("agent-empty-success-settlement");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let executable = root.join("agent/coder");
    let counter = root.join("empty-success-count");
    let counter_arg = crate::shell_single_quote(&counter.display().to_string());
    write_text_file(
        &executable,
        &format!("#!/bin/sh\nIFS= read -r envelope || exit 2\nprintf x >> {counter_arg}\n"),
    );
    set_file_mode(&executable, 0o755);
    let request =
        b"{\"op\":\"send\",\"id\":\"empty-1\",\"session\":\"default\",\"input\":\"run\"}\n";
    let send = || {
        let (mut client, mut socket) =
            UnixStream::pair().map_err(|_error| SocketRuntimeError::CannotReadFrame)?;
        client
            .write_all(request)
            .and_then(|()| client.shutdown(Shutdown::Write))
            .map_err(|_error| SocketRuntimeError::CannotWriteResponse)?;
        serve_agent_executable_socket_stream_once(
            &mut socket,
            None,
            direct_agent_runtime(&root, &view, &session_root, &executable),
        )
    };
    let first = ok!(send());
    let run = ok!(response_run(&first));
    let events = ok!(crate::support::columnar::read_text(
        &session_root.join("default"),
        crate::support::columnar::Stream::Events,
        1024 * 1024,
    ));
    let durable_done = ok!(done_statuses(&ok!(parse_jsonl(&events)), &run));
    let replay = ok!(send());
    let replay_done = ok!(done_statuses(&ok!(parse_jsonl(&replay.jsonl())), &run));
    assert_eq!(
        (durable_done, replay_done, ok!(fs::read_to_string(counter)),),
        (vec!["ok".to_owned()], vec!["ok".to_owned()], "x".to_owned(),)
    );
}
