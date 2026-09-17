use crate::{ConversionError, ProviderError, WireProtocol};
use serde_json::Value;

pub(super) fn provider_error(value: &Value) -> Option<ProviderError> {
    if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
        return Some(ProviderError::new("provider_error", message, false));
    }
    let reason = value
        .pointer("/promptFeedback/blockReason")
        .and_then(Value::as_str)?;
    Some(ProviderError::new(reason, "prompt blocked", false))
}

pub(super) fn invalid(field: &str) -> ConversionError {
    ConversionError::InvalidField {
        protocol: WireProtocol::Gemini,
        field: field.to_owned(),
    }
}
