fn write_agent_frames(stdout: &mut impl Write, run: &str, frames: &[String]) -> Result<(), String> {
    for frame in frames {
        if event_type(frame).is_some() {
            writeln!(stdout, "{frame}")
                .and_then(|()| stdout.flush())
                .map_err(|error| format!("cannot write output: {error}"))?;
        } else {
            write_model_text_or_tool_call(stdout, run, frame)
                .and_then(|()| stdout.flush())
                .map_err(|error| format!("cannot write output: {error}"))?;
        }
    }
    Ok(())
}

fn write_done_frames(stdout: &mut impl Write, frames: &[String]) -> Result<(), String> {
    for frame in frames {
        if event_type(frame).as_deref() == Some("done") {
            writeln!(stdout, "{frame}")
                .and_then(|()| stdout.flush())
                .map_err(|error| format!("cannot write output: {error}"))?;
        }
    }
    Ok(())
}

fn write_success_done_if_missing(
    stdout: &mut impl Write,
    run: &str,
    frames: &[String],
) -> Result<(), String> {
    if frames
        .iter()
        .any(|frame| event_type(frame).as_deref() == Some("done"))
    {
        return Ok(());
    }
    let done = serde_json::json!({
        "type": "done",
        "run": run,
        "status": "ok"
    })
    .to_string();
    writeln!(stdout, "{done}")
        .and_then(|()| stdout.flush())
        .map_err(|error| format!("cannot write output: {error}"))
}

fn write_agent_frames_for_tool_iteration(
    stdout: &mut impl Write,
    run: &str,
    frames: &[String],
    tool_call: &AgentToolCall,
) -> Result<(), String> {
    let mut wrote_tool_call = false;
    for frame in frames {
        if matches!(event_type(frame).as_deref(), Some("start" | "done")) {
            continue;
        }
        if tool_call_from_event_frame(frame)?.is_some() {
            writeln!(stdout, "{frame}")
                .and_then(|()| stdout.flush())
                .map_err(|error| format!("cannot write output: {error}"))?;
            wrote_tool_call = true;
        }
    }
    if !wrote_tool_call {
        write_tool_call_event(stdout, run, tool_call)
            .and_then(|()| stdout.flush())
            .map_err(|error| format!("cannot write output: {error}"))?;
    }
    Ok(())
}

fn write_tool_result_event(
    stdout: &mut impl Write,
    run: &str,
    tool_call: &AgentToolCall,
    result: &str,
) -> Result<(), String> {
    let args = tool_call_args_strings(tool_call);
    let event = serde_json::json!({
        "type": "message",
        "run": run,
        "role": "tool",
        "name": tool_call.name,
        "arguments": {
            "args": args
        },
        "content": [{
            "type": "tool_result",
            "tool_call_id": tool_call.id,
            "content": result
        }]
    })
    .to_string();
    if !inspect_event_stream_jsonl(&event).is_ok() {
        return Err("generated invalid tool result event".to_owned());
    }
    writeln!(stdout, "{event}").map_err(|error| format!("cannot write output: {error}"))
}

fn write_tool_result_fallback_response(
    stdout: &mut impl Write,
    run: &str,
    tool_call: &AgentToolCall,
    result: &str,
) -> Result<(), String> {
    let text = format!(
        "工具 `{}` 已执行，参数：{}\n\n输出：\n\n{}",
        tool_call.name,
        tool_call_args_json(tool_call),
        result
    );
    let message = serde_json::json!({
        "type": "message",
        "run": run,
        "role": "assistant",
        "content": [{
            "type": "text",
            "text": text
        }]
    })
    .to_string();
    let done = serde_json::json!({
        "type": "done",
        "run": run,
        "status": "ok"
    })
    .to_string();
    writeln!(stdout, "{message}")
        .and_then(|()| writeln!(stdout, "{done}"))
        .and_then(|()| stdout.flush())
        .map_err(|error| format!("cannot write output: {error}"))
}

fn missing_model_message(ctx_root: &Path, model: &str, model_path: &Path) -> String {
    if is_model_alias(model)
        && let Ok(target) = read_model_alias_target(ctx_root, model)
    {
        return format!("missing model: {model} -> {target}");
    }
    format!("missing model: {}", model_path.display())
}

fn write_model_start(stdout: &mut impl Write, run: &str, model: &str) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"start","run":{},"model":{}}}"#,
        json_string(run),
        json_string(model)
    )
}

fn write_model_delta(stdout: &mut impl Write, run: &str, text: &str) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"delta","run":{},"text":{}}}"#,
        json_string(run),
        json_string(text)
    )
}

fn write_model_usage(stdout: &mut impl Write, run: &str, usage: TokenUsage) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"usage","run":{},"input_tokens":{},"output_tokens":{}}}"#,
        json_string(run),
        usage.input_tokens,
        usage.output_tokens
    )
}

fn write_model_text_or_tool_call(stdout: &mut impl Write, run: &str, text: &str) -> io::Result<()> {
    if let Some(tool_call) = tool_call_from_text(text).map_err(io::Error::other)? {
        return write_tool_call_event(stdout, run, &tool_call);
    }
    write_model_delta(stdout, run, text)
}

fn write_tool_call_event(
    stdout: &mut impl Write,
    run: &str,
    tool_call: &AgentToolCall,
) -> io::Result<()> {
    let args = tool_call_args_strings(tool_call);
    let event = serde_json::json!({
        "type": "tool_call",
        "run": run,
        "id": tool_call.id,
        "name": tool_call.name,
        "arguments": {
            "args": args
        }
    })
    .to_string();
    writeln!(stdout, "{event}")
}

fn write_tool_start(stdout: &mut impl Write, run: &str, tool: &str) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"start","run":{},"tool":{}}}"#,
        json_string(run),
        json_string(tool)
    )
}

fn write_tool_done(stdout: &mut impl Write, run: &str, status: &str) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"done","run":{},"status":{}}}"#,
        json_string(run),
        json_string(status)
    )
}

fn write_tool_error(
    stdout: &mut impl Write,
    run: &str,
    code: &str,
    message: &str,
) -> io::Result<()> {
    write_error_event(stdout, run, code, message)?;
    write_tool_done(stdout, run, "error")
}

fn write_error_event(
    stdout: &mut impl Write,
    run: &str,
    code: &str,
    message: &str,
) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"error","run":{},"code":{},"message":{}}}"#,
        json_string(run),
        json_string(code),
        json_string(message)
    )
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_error| "\"\"".to_owned())
}
