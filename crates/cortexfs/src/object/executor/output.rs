use super::*;

pub(crate) fn write_hosted_agent_frames(
    stdout: &mut impl Write,
    run: &str,
    frames: &[String],
    streamed: bool,
) -> Result<(), ExecError> {
    for frame in frames {
        let kind = event_type(frame);
        if matches!(kind.as_deref(), Some("start" | "error" | "done"))
            || streamed
                && matches!(
                    kind.as_deref(),
                    Some("delta" | "reasoning_delta" | "usage" | "error")
                )
        {
            continue;
        }
        let write = match kind.as_deref() {
            Some("message" | "tool_call" | "delta" | "reasoning_delta" | "usage") => {
                writeln!(stdout, "{frame}")
            }
            None => write_model_text_or_tool_call(stdout, run, frame),
            Some(_) => continue,
        };
        write
            .and_then(|()| stdout.flush())
            .map_err(|error| ExecError::new(format!("cannot write output: {error}")))?;
    }
    Ok(())
}

pub(crate) fn write_agent_frames_for_tool_iteration(
    stdout: &mut impl Write,
    run: &str,
    frames: &[String],
    tool_call: &AgentToolCall,
) -> Result<(), ExecError> {
    let mut wrote_tool_call = false;
    for frame in frames {
        if matches!(event_type(frame).as_deref(), Some("start" | "done")) {
            continue;
        }
        if tool_call_from_event_frame(frame)?.is_some() {
            writeln!(stdout, "{frame}")
                .and_then(|()| stdout.flush())
                .map_err(|error| ExecError::new(format!("cannot write output: {error}")))?;
            wrote_tool_call = true;
        }
    }
    if !wrote_tool_call {
        write_tool_call_event(stdout, run, tool_call)
            .and_then(|()| stdout.flush())
            .map_err(|error| ExecError::new(format!("cannot write output: {error}")))?;
    }
    Ok(())
}

pub(crate) fn write_tool_result_event(
    stdout: &mut impl Write,
    run: &str,
    tool_call: &AgentToolCall,
    result: &str,
) -> Result<(), ExecError> {
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
    if !inspect_event_stream_jsonl(&format!("{event}\n")).is_ok() {
        return Err(ExecError::new("generated invalid tool result event"));
    }
    writeln!(stdout, "{event}")
        .map_err(|error| ExecError::new(format!("cannot write output: {error}")))
}

pub(crate) fn missing_model_message(ctx_root: &Path, model: &str, model_path: &Path) -> String {
    if is_model_alias(model)
        && let Ok(target) = read_model_alias_target(ctx_root, model)
    {
        return format!("missing model: {model} -> {target}");
    }
    format!("missing model: {}", model_path.display())
}

pub(crate) fn write_model_start(stdout: &mut impl Write, run: &str, model: &str) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"start","run":{},"model":{}}}"#,
        json_string(run),
        json_string(model)
    )
}

pub(crate) fn write_model_delta(stdout: &mut impl Write, run: &str, text: &str) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"delta","run":{},"text":{}}}"#,
        json_string(run),
        json_string(text)
    )
}

pub(crate) fn write_model_usage(
    stdout: &mut impl Write,
    run: &str,
    usage: TokenUsage,
) -> io::Result<()> {
    let cached_tokens = usage
        .cached_tokens
        .map_or_else(String::new, |value| format!(r#","cached_tokens":{value}"#));
    let cache_write_tokens = usage.cache_write_tokens.map_or_else(String::new, |value| {
        format!(r#","cache_write_tokens":{value}"#)
    });
    writeln!(
        stdout,
        r#"{{"type":"usage","run":{},"input_tokens":{},"output_tokens":{}{cached_tokens}{cache_write_tokens}}}"#,
        json_string(run),
        usage.input_tokens,
        usage.output_tokens
    )
}

pub(crate) fn write_model_text_or_tool_call(
    stdout: &mut impl Write,
    run: &str,
    text: &str,
) -> io::Result<()> {
    if let Some(tool_call) = tool_call_from_text(text).map_err(io::Error::other)? {
        return write_tool_call_event(stdout, run, &tool_call);
    }
    write_model_delta(stdout, run, text)
}

pub(crate) fn write_tool_call_event(
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

pub(crate) fn write_tool_start(stdout: &mut impl Write, run: &str, tool: &str) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"start","run":{},"tool":{}}}"#,
        json_string(run),
        json_string(tool)
    )
}

pub(crate) fn write_tool_done(stdout: &mut impl Write, run: &str, status: &str) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"done","run":{},"status":{}}}"#,
        json_string(run),
        json_string(status)
    )
}

pub(crate) fn write_tool_error(
    stdout: &mut impl Write,
    run: &str,
    code: &str,
    message: &str,
) -> io::Result<()> {
    write_error_event(stdout, run, code, message)?;
    write_tool_done(stdout, run, "error")
}

pub(crate) fn write_error_event(
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
