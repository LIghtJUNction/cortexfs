use crate::{ConversionError, ProviderError, Usage, WireProtocol};
use serde_json::{Map, Value};

pub(super) fn usage(map: Option<&Map<String, Value>>) -> Option<Usage> {
    let input = map
        .and_then(|value| value.get("promptTokenCount"))
        .and_then(Value::as_u64)?;
    let output = map
        .and_then(|value| value.get("candidatesTokenCount"))
        .and_then(Value::as_u64)?;
    Some(Usage {
        input_tokens: input,
        output_tokens: output,
        cached_tokens: None,
        reasoning_tokens: None,
    })
}
pub(super) fn provider_error(value: &Value) -> Option<ProviderError> {
    if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
        return Some(ProviderError::new("provider_error", message, false));
    }
    let reason = value
        .pointer("/promptFeedback/blockReason")
        .and_then(Value::as_str)?;
    Some(ProviderError::new(
        reason,
        format!("prompt blocked: {reason}"),
        false,
    ))
}
pub(super) fn invalid(field: &str) -> ConversionError {
    ConversionError::InvalidField {
        protocol: WireProtocol::Gemini,
        field: field.to_owned(),
    }
}
