fn shell_words(value: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escape = false;
    for character in value.chars() {
        if escape {
            word.push(character);
            escape = false;
            continue;
        }
        match (quote, character) {
            (_, '\\') => escape = true,
            (Some(active), candidate) if candidate == active => quote = None,
            (Some(_active), candidate) => word.push(candidate),
            (None, '\'' | '"') => quote = Some(character),
            (None, candidate) if candidate.is_whitespace() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            (None, candidate) => word.push(candidate),
        }
    }
    if escape {
        return Err("tool_call command ends with unfinished escape".to_owned());
    }
    if quote.is_some() {
        return Err("tool_call command has unterminated quote".to_owned());
    }
    if !word.is_empty() {
        words.push(word);
    }
    Ok(words)
}

fn validate_tool_call_arg_limits(args: &[String]) -> Result<(), String> {
    if args.len() > MAX_AGENT_TOOL_ARGC {
        return Err("tool_call args exceed argument count limit".to_owned());
    }
    let bytes = args
        .iter()
        .map(String::len)
        .try_fold(0_usize, usize::checked_add)
        .ok_or_else(|| "tool_call args exceed byte limit".to_owned())?;
    if bytes > MAX_AGENT_TOOL_ARG_BYTES {
        return Err("tool_call args exceed byte limit".to_owned());
    }
    Ok(())
}

fn validate_agent_tsh_args(args: &[OsString]) -> Result<(), String> {
    if args.is_empty() {
        return Err("tool_call args for tsh cannot be empty".to_owned());
    }
    let Some(first) = args.first() else {
        return Err("tool_call args for tsh cannot be empty".to_owned());
    };
    let Some(first) = first.to_str() else {
        return Err("tool_call args must be valid UTF-8".to_owned());
    };
    if matches!(first, "--root" | "-r") {
        return Err("tool_call args cannot override tsh root".to_owned());
    }
    if first == "tsh" {
        return Err("tool_call args for tsh must not include the tsh program name".to_owned());
    }
    Ok(())
}

fn tool_denial_message(name: &str, denial: ToolExecutionDenial) -> String {
    format!("cannot execute tool:{name}: {}", denial.errno())
}

fn trim_tool_result(result: &str) -> String {
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

fn trim_tool_context_to_limit(context: &mut String) {
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
