use super::*;

pub(crate) fn normalize_agent_model_frame(frame: &str, run: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(frame) else {
        return frame.to_owned();
    };
    if value.get("type").and_then(Value::as_str).is_some() && value.get("run").is_none() {
        let Some(object) = value.as_object_mut() else {
            return frame.to_owned();
        };
        object.insert("run".to_owned(), Value::String(run.to_owned()));
        return value.to_string();
    }
    frame.to_owned()
}

pub(crate) fn should_write_streamed_model_frame(frame: &str, suppress_error: bool) -> bool {
    if serde_json::from_str::<Value>(frame)
        .ok()
        .is_some_and(|value| value.get("recoverable").and_then(Value::as_bool) == Some(true))
    {
        return true;
    }
    match event_type(frame).as_deref() {
        Some("delta" | "reasoning_delta" | "usage") => true,
        Some("error") => !suppress_error,
        _ => false,
    }
}

pub(crate) fn frames_have_error(frames: &[String]) -> bool {
    frames.iter().any(|frame| {
        let Ok(value) = serde_json::from_str::<Value>(frame) else {
            return false;
        };
        value.get("type").and_then(Value::as_str) == Some("error")
            && value.get("recoverable").and_then(Value::as_bool) != Some(true)
    })
}

pub(crate) fn event_type(frame: &str) -> Option<String> {
    serde_json::from_str::<Value>(frame)
        .ok()
        .and_then(|value| value.get("type").and_then(Value::as_str).map(str::to_owned))
}
