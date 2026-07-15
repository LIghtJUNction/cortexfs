use super::*;

pub(crate) fn shell_words(value: &str) -> Result<Vec<String>, ExecError> {
    parse_shell_words!(
        value,
        "tool_call command ends with unfinished escape".to_owned(),
        "tool_call command has unterminated quote".to_owned()
    )
    .map_err(ExecError::new)
}

pub(crate) fn validate_tool_call_arg_limits(args: &[String]) -> Result<(), ExecError> {
    if args.len() > MAX_AGENT_TOOL_ARGC {
        return Err(ExecError::new("tool_call args exceed argument count limit"));
    }
    let bytes = args
        .iter()
        .map(String::len)
        .try_fold(0_usize, usize::checked_add)
        .ok_or_else(|| ExecError::new("tool_call args exceed byte limit"))?;
    if bytes > MAX_AGENT_TOOL_ARG_BYTES {
        return Err(ExecError::new("tool_call args exceed byte limit"));
    }
    Ok(())
}

pub(crate) fn validate_agent_tsh_args(args: &[OsString]) -> Result<(), ExecError> {
    if args.is_empty() {
        return Err(ExecError::new("tool_call args for tsh cannot be empty"));
    }
    let Some(first) = args.first() else {
        return Err(ExecError::new("tool_call args for tsh cannot be empty"));
    };
    let Some(first) = first.to_str() else {
        return Err(ExecError::new("tool_call args must be valid UTF-8"));
    };
    if matches!(first, "--root" | "-r") {
        return Err(ExecError::new("tool_call args cannot override tsh root"));
    }
    if first == "tsh" {
        return Err(ExecError::new(
            "tool_call args for tsh must not include the tsh program name",
        ));
    }
    Ok(())
}

pub(crate) fn tool_denial_message(name: &str, denial: ToolExecutionDenial) -> String {
    format!("cannot execute tool:{name}: {}", denial.errno())
}

pub(crate) fn trim_tool_result(result: &str) -> String {
    let mut result = result.to_owned();
    if result.len() > MAX_TOOL_RESULT_CHARS {
        let marker = "\n[truncated]\n";
        let mut end = MAX_TOOL_RESULT_CHARS.saturating_sub(marker.len());
        while !result.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        result.truncate(end);
        result.push_str(marker);
    }
    result
}

pub(crate) fn trim_tool_context_to_limit(context: &mut String) {
    if context.len() <= MAX_AGENT_TOOL_CONTEXT_BYTES {
        return;
    }
    let marker = "[earlier tool context truncated]\n\n";
    let budget = MAX_AGENT_TOOL_CONTEXT_BYTES.saturating_sub(marker.len());
    let mut start = context.len().saturating_sub(budget);
    while !context.is_char_boundary(start) {
        start = start.saturating_add(1);
    }
    let tail = context.get(start..).unwrap_or_default();
    if let Some(offset) = tail.find("\n\nTool result ") {
        start = start.saturating_add(offset).saturating_add(2);
    }
    let mut trimmed = String::with_capacity(marker.len() + context.len().saturating_sub(start));
    trimmed.push_str(marker);
    trimmed.push_str(context.get(start..).unwrap_or_default());
    if trimmed.len() > MAX_AGENT_TOOL_CONTEXT_BYTES {
        let mut retry_start = trimmed.len().saturating_sub(MAX_AGENT_TOOL_CONTEXT_BYTES);
        while !trimmed.is_char_boundary(retry_start) {
            retry_start = retry_start.saturating_add(1);
        }
        let tail = trimmed.get(retry_start..).unwrap_or_default().to_owned();
        trimmed.clear();
        trimmed.push_str(&tail);
    }
    *context = trimmed;
}
