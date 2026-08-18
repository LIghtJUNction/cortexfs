use crate::{ConversionError, WireProtocol};

/// Native request IR selected by a wire protocol dialect.
#[derive(Clone, Debug)]
pub enum NativeRequest<'a> {
    OpenAiChat(crate::OpenAiChatRequest<'a>),
    OpenAiResponses(crate::OpenAiResponsesRequest<'a>),
    Gemini(crate::GeminiRequest<'a>),
    Anthropic(crate::AnthropicRequest<'a>),
}

/// Path used by one request conversion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgePath {
    Identity,
    Direct,
    ViaIr,
}

/// Converted target bytes and the path used to produce them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranscodedRequest {
    pub bytes: Vec<u8>,
    pub path: BridgePath,
}

/// Decodes a native request into the owned `CortexFS` semantic IR.
pub fn decode_model_request(
    protocol: WireProtocol,
    input: &[u8],
) -> Result<crate::ModelRequest, ConversionError> {
    crate::semantic::decode_request(protocol, input)
}

/// Encodes the owned `CortexFS` semantic IR into a native request dialect.
pub fn encode_model_request(
    protocol: WireProtocol,
    request: &crate::ModelRequest,
) -> Result<Vec<u8>, ConversionError> {
    crate::semantic::encode_request(protocol, request)
}

/// Parses one request into its protocol-specific borrowed IR.
pub fn decode_native_request(
    protocol: WireProtocol,
    input: &[u8],
) -> Result<NativeRequest<'_>, ConversionError> {
    match protocol {
        WireProtocol::OpenAiChat => parse(protocol, input).map(NativeRequest::OpenAiChat),
        WireProtocol::OpenAiResponses => parse(protocol, input).map(NativeRequest::OpenAiResponses),
        WireProtocol::Gemini => parse(protocol, input).map(NativeRequest::Gemini),
        WireProtocol::Anthropic => parse(protocol, input).map(NativeRequest::Anthropic),
    }
}

/// Converts request bytes, selecting a direct adapter before any IR fallback.
pub fn transcode_request(
    source: WireProtocol,
    target: WireProtocol,
    input: &[u8],
) -> Result<TranscodedRequest, ConversionError> {
    if source == target {
        return Ok(TranscodedRequest {
            bytes: input.to_vec(),
            path: BridgePath::Identity,
        });
    }
    let direct = match (source, target) {
        (WireProtocol::OpenAiChat, WireProtocol::Gemini) => crate::direct::openai_to_gemini(input),
        (WireProtocol::Gemini, WireProtocol::OpenAiChat) => crate::direct::gemini_to_openai(input),
        _ => Err(ConversionError::UnsupportedField {
            protocol: source,
            field: format!("direct route to {target}"),
        }),
    };
    if let Ok(bytes) = direct {
        return Ok(TranscodedRequest {
            bytes,
            path: BridgePath::Direct,
        });
    }
    let request = decode_model_request(source, input)?;
    Ok(TranscodedRequest {
        bytes: encode_model_request(target, &request)?,
        path: BridgePath::ViaIr,
    })
}

fn parse<'a, T>(protocol: WireProtocol, input: &'a [u8]) -> Result<T, ConversionError>
where
    T: serde::Deserialize<'a>,
{
    serde_json::from_slice(input).map_err(|error| ConversionError::InvalidJson {
        protocol,
        detail: error.to_string(),
    })
}
