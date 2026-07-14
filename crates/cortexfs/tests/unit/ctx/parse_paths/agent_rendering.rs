#[test]
fn debug_timing_diagnostic_is_readable() {
    let value = serde_json::json!({
        "type": "debug",
        "stage": "first_model_frame",
        "elapsed_ms": 42
    });

    assert_eq!(
        debug_timing_diagnostic(&value),
        Some("[debug timing] +42ms first_model_frame".to_owned())
    );
}

#[test]
fn buffered_agent_renderer_shows_tool_args_result_preview_and_usage() {
    let input = concat!(
        r#"{"type":"tool_call","run":"r1","id":"call-1","name":"tsh","arguments":{"args":["shell.exec","cargo test -p cortexfs"]}}"#,
        "\n",
        r#"{"type":"message","run":"r1","role":"tool","name":"tsh","arguments":{"args":["shell.exec","cargo test -p cortexfs"]},"content":[{"type":"tool_result","tool_call_id":"call-1","content":"running 2 tests\nok\n"}]}"#,
        "\n",
        r#"{"type":"usage","run":"r1","input_tokens":17,"output_tokens":5}"#,
        "\n"
    );

    let rendered = collect_agent_events_buffered(std::io::Cursor::new(input));

    assert!(matches!(
        rendered,
        Ok(ref rendered)
            if rendered.output.is_empty()
                && rendered.diagnostics.iter().any(|line| line.contains("tool tsh running shell.exec \"cargo test -p cortexfs\""))
                && rendered.diagnostics.iter().any(|line| line.contains("tool tsh done 19 bytes ~5 tokens"))
                && rendered.diagnostics.iter().any(|line| line.contains("args: shell.exec \"cargo test -p cortexfs\""))
                && rendered.diagnostics.iter().any(|line| line.contains("result: running 2 tests\nok"))
                && rendered.diagnostics.iter().any(|line| line.contains("tokens in +17/17 out +5/5"))
                && rendered.exit_code == 0
    ));
}

#[test]
fn buffered_agent_renderer_rejects_too_much_output() {
    let chunk = "x".repeat(1024);
    let mut input = String::new();
    for _index in 0..(MAX_BUFFERED_AGENT_RENDERED_BYTES / chunk.len() + 2) {
        let _ignored = writeln!(input, "{{\"type\":\"delta\",\"text\":\"{chunk}\"}}");
    }

    let rendered = collect_agent_events_buffered(std::io::Cursor::new(input));

    assert!(matches!(rendered, Err(ref error) if error.message.contains("agent output exceeds")));
}

#[test]
fn streaming_agent_renderer_rejects_oversized_frame() {
    let input = format!("{}\n", "x".repeat(MAX_SOCKET_FRAME_BYTES));

    let rendered = render_agent_event_lines(std::io::Cursor::new(input), None, None);

    assert!(matches!(rendered, Err(ref error) if error.message.contains("cannot read socket response")));
}

#[test]
fn streaming_agent_renderer_rejects_too_much_response_data() {
    let frame = "{\"type\":\"ignored\"}\n";
    let input = frame.repeat(MAX_AGENT_RESPONSE_BYTES / frame.len() + 1);

    let rendered = render_agent_event_lines(std::io::Cursor::new(input), None, None);

    assert!(matches!(rendered, Err(ref error) if error.message.contains("agent response exceeds")));
}

#[test]
fn streaming_agent_renderer_rejects_too_many_events() {
    let input = "{\"type\":\"ignored\"}\n".repeat(MAX_AGENT_EVENTS + 1);

    let rendered = render_agent_event_lines(std::io::Cursor::new(input), None, None);

    assert!(matches!(rendered, Err(ref error) if error.message.contains("agent response exceeds")));
}

#[test]
fn buffered_agent_renderer_rejects_oversized_frame_before_rendering() {
    let input = format!("{}\n", "x".repeat(MAX_SOCKET_FRAME_BYTES));

    let rendered = collect_agent_events_buffered(std::io::Cursor::new(input));

    assert!(matches!(rendered, Err(ref error) if error.message.contains("cannot read socket response")));
}

#[test]
fn buffered_agent_renderer_rejects_too_many_events() {
    let input = "{\"type\":\"ignored\"}\n".repeat(MAX_BUFFERED_AGENT_EVENTS + 1);

    let rendered = collect_agent_events_buffered(std::io::Cursor::new(input));

    assert!(matches!(rendered, Err(ref error) if error.message.contains("buffered events")));
}

#[test]
fn buffered_agent_renderer_rejects_too_many_diagnostics() {
    let input = "{\"type\":\"tool_call\",\"name\":\"tsh\"}\n"
        .repeat(MAX_BUFFERED_AGENT_DIAGNOSTICS + 1);

    let rendered = collect_agent_events_buffered(std::io::Cursor::new(input));

    assert!(matches!(rendered, Err(ref error) if error.message.contains("buffered diagnostics")));
}

#[test]
fn interruptible_agent_renderer_preserves_partial_frame_across_timeout() {
    let pair = std::os::unix::net::UnixStream::pair();
    assert!(pair.is_ok());
    let Ok((reader, mut writer)) = pair else {
        return;
    };
    assert!(reader
        .set_read_timeout(Some(std::time::Duration::from_millis(10)))
        .is_ok());
    let writer_thread = std::thread::spawn(move || {
        assert!(std::io::Write::write_all(&mut writer, br#"{"type":"error","#).is_ok());
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(
            std::io::Write::write_all(
                &mut writer,
                br#""code":"EFAIL","message":"slow split frame"}"#,
            )
            .is_ok()
        );
        assert!(std::io::Write::write_all(&mut writer, b"\n").is_ok());
    });
    let interrupted = std::sync::atomic::AtomicBool::new(false);

    let rendered =
        collect_agent_events_buffered_interruptible(std::io::BufReader::new(reader), &interrupted);

    assert!(writer_thread.join().is_ok());
    assert!(matches!(
        rendered,
        Ok(ref rendered)
            if rendered.output.is_empty()
                && rendered.diagnostics == vec!["error EFAIL: slow split frame".to_owned()]
                && rendered.exit_code == 1
                && !rendered.interrupted
    ));
}

#[test]
fn interruptible_agent_renderer_returns_on_interrupt_flag() {
    let pair = std::os::unix::net::UnixStream::pair();
    assert!(pair.is_ok());
    let Ok((reader, _writer)) = pair else {
        return;
    };
    assert!(reader
        .set_read_timeout(Some(std::time::Duration::from_millis(1)))
        .is_ok());
    let interrupted = std::sync::atomic::AtomicBool::new(true);

    let rendered =
        collect_agent_events_buffered_interruptible(std::io::BufReader::new(reader), &interrupted);

    assert!(matches!(
        rendered,
        Ok(ref rendered)
            if rendered.output.is_empty()
                && rendered.diagnostics.is_empty()
                && rendered.exit_code == 0
                && rendered.interrupted
    ));
}
