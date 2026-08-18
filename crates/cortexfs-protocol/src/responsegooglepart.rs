use crate::{ConversionError, Usage, WireProtocol};
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

pub(super) fn invalid(field: &str) -> ConversionError {
    ConversionError::InvalidField {
        protocol: WireProtocol::Gemini,
        field: field.to_owned(),
    }
}
pub(super) fn missing(field: &str) -> ConversionError {
    ConversionError::MissingField {
        protocol: WireProtocol::Gemini,
        field: field.to_owned(),
    }
}
