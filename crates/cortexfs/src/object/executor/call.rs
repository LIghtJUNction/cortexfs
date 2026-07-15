use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentToolCall {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) args: Vec<OsString>,
}

pub(crate) fn first_tool_call(frames: &[String]) -> Result<Option<AgentToolCall>, ExecError> {
    for frame in frames {
        if let Some(call) = tool_call_from_event_frame(frame)? {
            return Ok(Some(call));
        }
        if let Some(text) = event_text(frame)
            && let Some(call) = tool_call_from_text(&text)?
        {
            return Ok(Some(call));
        }
    }
    Ok(None)
}

pub(crate) fn tool_call_signature(tool_call: &AgentToolCall) -> String {
    let args = tool_call_args_strings(tool_call).join("\u{1f}");
    format!("{}\u{1e}{args}", tool_call.name)
}

pub(crate) fn tool_call_args_strings(tool_call: &AgentToolCall) -> Vec<String> {
    tool_call
        .args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

pub(crate) fn tool_call_args_json(tool_call: &AgentToolCall) -> String {
    serde_json::to_string(&tool_call_args_strings(tool_call))
        .unwrap_or_else(|_error| "[]".to_owned())
}

pub(crate) fn tool_call_from_event_frame(frame: &str) -> Result<Option<AgentToolCall>, ExecError> {
    let Ok(value) = serde_json::from_str::<Value>(frame) else {
        return Ok(None);
    };
    if value.get("type").and_then(Value::as_str) != Some("tool_call") {
        return Ok(None);
    }
    agent_tool_call_from_value(&value)
}

pub(crate) fn event_text(frame: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(frame).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("delta") {
        return None;
    }
    value.get("text").and_then(Value::as_str).map(str::to_owned)
}

pub(crate) fn tool_call_from_text(text: &str) -> Result<Option<AgentToolCall>, ExecError> {
    let trimmed = text.trim();
    if !trimmed.starts_with('{') {
        return Ok(None);
    }
    let mut deserializer = serde_json::Deserializer::from_str(trimmed);
    let Ok(value) = Value::deserialize(&mut deserializer) else {
        return Ok(None);
    };
    if value.get("type").and_then(Value::as_str) != Some("tool_call") {
        return Ok(None);
    }
    agent_tool_call_from_value(&value)
}

pub(crate) fn agent_tool_call_from_value(
    value: &Value,
) -> Result<Option<AgentToolCall>, ExecError> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| ExecError::new("tool_call missing id"))?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| ExecError::new("tool_call missing name"))?;
    if !is_object_name(id) {
        return Err(ExecError::new(format!("invalid tool_call id: {id}")));
    }
    if !is_object_name(name) {
        return Err(ExecError::new(format!("invalid tool_call name: {name}")));
    }
    let args = tool_call_args(value.get("arguments"))?;
    Ok(Some(AgentToolCall {
        id: id.to_owned(),
        name: name.to_owned(),
        args,
    }))
}

pub(crate) fn tool_call_args(arguments: Option<&Value>) -> Result<Vec<OsString>, ExecError> {
    let args = match arguments {
        None => Vec::new(),
        Some(arguments) => {
            if let Some(args) = arguments.get("args").or_else(|| arguments.get("argv")) {
                json_string_array(args)?
            } else if let Some(command) = arguments.get("command").and_then(Value::as_str) {
                shell_words(command)?
            } else if let Some(input) = arguments.get("input").and_then(Value::as_str) {
                vec![input.to_owned()]
            } else if let Some(value) = arguments.as_str() {
                shell_words(value)?
            } else {
                return Err(ExecError::new(
                    "tool_call arguments must contain args, argv, command, or input",
                ));
            }
        }
    };
    validate_tool_call_arg_limits(&args)?;
    Ok(args.into_iter().map(OsString::from).collect())
}

pub(crate) fn json_string_array(value: &Value) -> Result<Vec<String>, ExecError> {
    let Some(values) = value.as_array() else {
        return Err(ExecError::new("tool_call args must be an array"));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| ExecError::new("tool_call args must be strings"))
        })
        .collect()
}
