use crate::{ConversionError, ModelRequest, WireProtocol};
use serde_json::{Value, value::RawValue};

/// Decodes one native request into the owned semantic model IR.
pub(super) fn decode_request(
    protocol: WireProtocol,
    input: &[u8],
) -> Result<ModelRequest, ConversionError> {
    match protocol {
        WireProtocol::OpenAiChat => crate::decodeopenai::request(input),
        WireProtocol::OpenAiResponses => crate::decoderesponses::request(input),
        WireProtocol::Gemini => crate::decodegoogle::request(input),
        WireProtocol::Anthropic => crate::decodeanthropic::request(input),
    }
}

/// Encodes the semantic model IR into one native request dialect.
pub(super) fn encode_request(
    protocol: WireProtocol,
    request: &ModelRequest,
) -> Result<Vec<u8>, ConversionError> {
    request.validate()?;
    match protocol {
        WireProtocol::OpenAiChat => crate::encodeopenai::request(request),
        WireProtocol::OpenAiResponses => crate::encoderesponses::request(request),
        WireProtocol::Gemini => crate::encodegoogle::request(request),
        WireProtocol::Anthropic => crate::encodeanthropic::request(request),
    }
}

pub(super) fn parse<'a, T: serde::Deserialize<'a>>(
    protocol: WireProtocol,
    input: &'a [u8],
) -> Result<T, ConversionError> {
    serde_json::from_slice(input).map_err(|error| ConversionError::InvalidJson {
        protocol,
        detail: error.to_string(),
    })
}

pub(super) fn raw_value(
    protocol: WireProtocol,
    field: &str,
    raw: &RawValue,
) -> Result<Value, ConversionError> {
    serde_json::from_str(raw.get()).map_err(|_error| ConversionError::InvalidField {
        protocol,
        field: field.to_owned(),
    })
}

pub(super) fn json_value(
    protocol: WireProtocol,
    field: &str,
    value: &str,
) -> Result<Value, ConversionError> {
    serde_json::from_str(value).map_err(|_error| ConversionError::InvalidField {
        protocol,
        field: field.to_owned(),
    })
}
