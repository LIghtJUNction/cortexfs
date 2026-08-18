use crate::{ConversionError, ModelEvent, WireProtocol};

pub(super) fn encode(
    protocol: WireProtocol,
    events: &[ModelEvent],
) -> Result<Vec<u8>, ConversionError> {
    match protocol {
        WireProtocol::OpenAiChat => crate::responseopenai::encode(events),
        WireProtocol::OpenAiResponses => crate::responseresponses::encode(events),
        WireProtocol::Gemini => crate::responsegoogle::encode(events),
        WireProtocol::Anthropic => crate::responseanthropic::encode(events),
    }
}
