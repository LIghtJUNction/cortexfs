use serde_json::Value;

use super::DingTalkError;

pub(super) fn root(payload: &str) -> Result<Value, DingTalkError> {
    let value: Value = serde_json::from_str(payload)?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(DingTalkError::Protocol(
            "gateway frame is not an object".to_owned(),
        ))
    }
}

pub(super) fn frame_type(root: &Value) -> Option<&str> {
    root.get("type").and_then(Value::as_str)
}
