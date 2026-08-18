use crate::{ConversionError, ModelEvent, WireProtocol};

pub(super) fn decode(
    protocol: WireProtocol,
    input: &[u8],
) -> Result<Vec<ModelEvent>, ConversionError> {
    match protocol {
        WireProtocol::OpenAiChat => crate::responseopenai::decode(input),
        WireProtocol::OpenAiResponses => crate::responseresponses::decode(input),
        WireProtocol::Gemini => crate::responsegoogle::decode(input),
        WireProtocol::Anthropic => crate::responseanthropic::decode(input),
    }
}
