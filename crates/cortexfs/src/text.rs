pub fn json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len().saturating_add(2));
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "\\u{:04x}", u32::from(character));
            }
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

pub fn audit_cost_content(billable_events: usize, tool_calls: usize, agent_tasks: usize) -> String {
    const MICRO_USD_PER_EVENT: usize = 1;
    let micro_usd = billable_events.saturating_mul(MICRO_USD_PER_EVENT);
    format!(
        "usd={}\nbillable_events={billable_events}\ndrained={billable_events}\ntool_calls={tool_calls}\nagent_tasks={agent_tasks}\n",
        micros_to_usd(micro_usd),
    )
}

fn micros_to_usd(micro_usd: usize) -> String {
    let whole = micro_usd / 1_000_000;
    let fractional = micro_usd % 1_000_000;
    format!("{whole}.{fractional:06}")
}

pub fn external_subject(body: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(body).ok()?;
    value
        .get("subject")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            value
                .get("messages")
                .and_then(serde_json::Value::as_array)
                .and_then(|messages| {
                    messages
                        .iter()
                        .rev()
                        .find_map(|message| message.get("subject")?.as_str().map(ToOwned::to_owned))
                })
        })
}

pub fn external_display_name(body: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(body).ok()?;
    value
        .get("display_name")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            value
                .get("messages")
                .and_then(serde_json::Value::as_array)
                .and_then(|messages| {
                    messages.iter().rev().find_map(|message| {
                        message.get("display_name")?.as_str().map(ToOwned::to_owned)
                    })
                })
        })
}

pub fn redact_dsn(dsn: &str) -> String {
    let bytes = dsn.as_bytes();
    let Some(scheme_end) = bytes.windows(3).position(|window| window == b"://") else {
        return dsn.to_owned();
    };
    let authority_start = scheme_end.saturating_add(3);
    let Some(after_scheme) = bytes.get(authority_start..) else {
        return dsn.to_owned();
    };
    let authority_end = after_scheme
        .iter()
        .position(|byte| matches!(*byte, b'/' | b'?'))
        .map_or(bytes.len(), |offset| authority_start.saturating_add(offset));
    let Some(authority) = bytes.get(authority_start..authority_end) else {
        return dsn.to_owned();
    };
    let Some(at_index) = authority.iter().rposition(|byte| *byte == b'@') else {
        return dsn.to_owned();
    };
    let Some(userinfo) = authority.get(..at_index) else {
        return dsn.to_owned();
    };
    let Some(colon_index) = userinfo.iter().rposition(|byte| *byte == b':') else {
        return dsn.to_owned();
    };
    let mut redacted = Vec::with_capacity(bytes.len());
    let Some(prefix) = bytes.get(..authority_start) else {
        return dsn.to_owned();
    };
    let Some(user_prefix) = authority.get(..colon_index.saturating_add(1)) else {
        return dsn.to_owned();
    };
    let Some(host_suffix) = authority.get(at_index.saturating_add(1)..) else {
        return dsn.to_owned();
    };
    let Some(path_suffix) = bytes.get(authority_end..) else {
        return dsn.to_owned();
    };
    redacted.extend_from_slice(prefix);
    redacted.extend_from_slice(user_prefix);
    redacted.extend_from_slice(b"***@");
    redacted.extend_from_slice(host_suffix);
    redacted.extend_from_slice(path_suffix);
    String::from_utf8(redacted)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
}

pub fn user_content(request_body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(request_body)
        .ok()
        .and_then(|body| {
            body.get("messages")
                .and_then(serde_json::Value::as_array)
                .and_then(|messages| {
                    messages.iter().rev().find(|message| {
                        message
                            .get("role")
                            .and_then(serde_json::Value::as_str)
                            .is_some_and(|role| role == "user")
                    })
                })
                .and_then(|message| message.get("content"))
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| request_body.to_owned())
}

pub fn assistant_content(response_body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(response_body)
        .ok()
        .and_then(|body| {
            body.get("choices")
                .and_then(serde_json::Value::as_array)
                .and_then(|choices| choices.first())
                .and_then(|choice| choice.get("message"))
                .and_then(|message| message.get("content"))
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| response_body.to_owned())
}
