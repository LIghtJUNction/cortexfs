use super::*;
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Instant;

fn write_cancellable_partial_agent(executable: &Path, after_delta: &str) {
    // Bounded wait: TERM ends promptly; integer sleep stays portable on /bin/sh.
    write_text_file(
        executable,
        &r#"#!/bin/sh
trap 'exit 0' TERM INT
sleep 3
printf '{"type":"delta","run":"%s","text":"partial"}\n' "$CTX_RUN_ID"
touch "$CTX_SOURCE/cancel-ready"
sleep 5
printf '{"type":"done","run":"%s","status":"ok"}\n' "$CTX_RUN_ID"
"#
        .replace("sleep 5", after_delta),
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
    let delta_deadline = Instant::now() + Duration::from_secs(20);
    reader
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(100)))
        .ok()?;
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => return None,
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) && Instant::now() < delta_deadline => {}
            Err(_) => return None,
        }
        if line.is_empty() {
            continue;
        }
        response.push_str(&line);
        let frame: Value = serde_json::from_str(&line).ok()?;
        if json_str(&frame, "type") != Some("delta") {
            continue;
        }
        let ready_deadline = Instant::now() + Duration::from_secs(10);
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
    for after_delta in [
        "sleep 5",
        "exec 1>&-\nsleep 5",
        "/usr/bin/timeout 5 /usr/bin/yes ''",
        "sleep 5 >/dev/null &\nexit 0",
    ] {
        let root = reference_tree("agent-partial-delta-cancel");
        let session_root = agent_session_root(&root, "executor");
        let view = ok!(derive_agent_runtime_view(&root, "executor"));
        let executable = root.join("agent/executor");
        write_cancellable_partial_agent(&executable, after_delta);

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
        let started = Instant::now();
        let outcome = serve_agent_executable_socket_stream_once(
            &mut socket,
            None,
            direct_agent_runtime(&root, &view, &session_root, &executable),
        );

        let joined = cancel.join();
        assert!(
            started.elapsed() < Duration::from_secs(6),
            "cancel stalled: {after_delta}"
        );
        assert!(
            matches!(&joined, Ok(Some((true, _, _)))),
            "cancel observer failed: {joined:?}"
        );
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
}

#[test]
fn wait_capped_child_output_timeout_does_not_wait_for_escaped_pipe_holder()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let pid_file = temp.path().join("escaped.pid");
    let mut child = Command::new("/usr/bin/sh")
        .arg("-c")
        .arg(r#"/usr/bin/setsid /usr/bin/sh -c 'echo $$ > "$PID_FILE"; sleep 30' & sleep 30"#)
        .env("PID_FILE", &pid_file)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    let escaped_pid = {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok(text) = fs::read_to_string(&pid_file)
                && let Ok(pid) = text.trim().parse::<i32>()
                && pid > 1
            {
                break pid;
            }
            if Instant::now() >= deadline {
                crate::support::process::terminate_process_group(&mut child);
                let _ignored = child.wait();
                return Err("escaped grandchild did not publish its PID".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
    };
    let (finished_tx, finished_rx) = mpsc::sync_channel(1);
    let waiter = thread::spawn(move || {
        let result = crate::support::process::wait_capped_child_output(
            &mut child,
            crate::support::process::CappedOutputWait {
                max_output_bytes: 64,
                timeout: Duration::from_millis(100),
                capture_stderr: false,
                drain_timeout: None,
                terminate_group_after_exit: false,
            },
            || false,
        );
        let _ignored = finished_tx.send(());
        result
    });

    let bounded = finished_rx.recv_timeout(Duration::from_secs(2));
    crate::support::process::signal_process_group(escaped_pid, nix::sys::signal::Signal::SIGKILL);
    let result = waiter
        .join()
        .map_err(|_panic| "capped-output waiter panicked")?;
    bounded.map_err(|error| format!("capped-output wait was not bounded: {error}"))?;
    assert!(matches!(
        result,
        Err(crate::support::process::CappedOutputError::TimedOut)
    ));
    Ok(())
}
