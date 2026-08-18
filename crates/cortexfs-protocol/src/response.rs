use crate::{BridgePath, ConversionError, ModelEvent, WireProtocol};

/// Converted response bytes and the path used to produce them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranscodedResponse {
    pub bytes: Vec<u8>,
    pub path: BridgePath,
}

/// Decodes one non-streaming native response into normalized `CortexFS` events.
pub fn decode_response_events(
    protocol: WireProtocol,
    input: &[u8],
) -> Result<Vec<ModelEvent>, ConversionError> {
    crate::responsedecode::decode(protocol, input)
}

/// Encodes normalized `CortexFS` events as one non-streaming native response.
pub fn encode_response_events(
    protocol: WireProtocol,
    events: &[ModelEvent],
) -> Result<Vec<u8>, ConversionError> {
    crate::responseencode::encode(protocol, events)
}

/// Converts a complete response body through the normalized event IR.
pub fn transcode_response(
    source: WireProtocol,
    target: WireProtocol,
    input: &[u8],
) -> Result<TranscodedResponse, ConversionError> {
    if source == target {
        return Ok(TranscodedResponse {
            bytes: input.to_vec(),
            path: BridgePath::Identity,
        });
    }
    let events = decode_response_events(source, input)?;
    Ok(TranscodedResponse {
        bytes: encode_response_events(target, &events)?,
        path: BridgePath::ViaIr,
    })
}
