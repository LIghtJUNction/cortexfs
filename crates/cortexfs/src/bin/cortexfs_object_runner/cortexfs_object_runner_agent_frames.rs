fn normalize_agent_model_frame(frame: &str, run: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(frame) else {
        return frame.to_owned();
    };
    if value.get("type").and_then(Value::as_str) == Some("error")
        && value.get("run").is_none()
        && let Some(object) = value.as_object_mut()
    {
        object.insert("run".to_owned(), Value::String(run.to_owned()));
        return value.to_string();
    }
    frame.to_owned()
}

fn should_write_streamed_model_frame(frame: &str, suppress_error: bool) -> bool {
    match event_type(frame).as_deref() {
        Some("delta" | "reasoning_delta" | "usage") => true,
        Some("error") => !suppress_error,
        _ => false,
    }
}

fn frames_have_error(frames: &[String]) -> bool {
    frames
        .iter()
        .any(|frame| event_type(frame).as_deref() == Some("error"))
}

fn frames_have_visible_assistant_response(frames: &[String]) -> bool {
    frames.iter().any(|frame| {
        let Ok(value) = serde_json::from_str::<Value>(frame) else {
            return !frame.trim().is_empty();
        };
        match value.get("type").and_then(Value::as_str) {
            Some("delta") => value
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.trim().is_empty()),
            Some("message") if value.get("role").and_then(Value::as_str) == Some("assistant") => {
                message_has_visible_text(&value)
            }
            _ => false,
        }
    })
}

fn message_has_visible_text(value: &Value) -> bool {
    value
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("type").and_then(Value::as_str) == Some("text")
                    && item
                        .get("text")
                        .and_then(Value::as_str)
                        .is_some_and(|text| !text.trim().is_empty())
            })
        })
}

fn event_type(frame: &str) -> Option<String> {
    serde_json::from_str::<Value>(frame)
        .ok()
        .and_then(|value| value.get("type").and_then(Value::as_str).map(str::to_owned))
}
