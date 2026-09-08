use crate::{ConversionError, ModelEvent, WireProtocol};

pub(super) fn encode(
    protocol: WireProtocol,
    events: &[ModelEvent],
) -> Result<Vec<u8>, ConversionError> {
    let usage = events.iter().rev().find_map(|event| match *event {
        ModelEvent::Usage { ref usage, .. } => Some(usage),
        _ => None,
    });
    if protocol != WireProtocol::Anthropic
        && usage.is_some_and(|usage| {
            usage
                .input_tokens
                .checked_add(usage.output_tokens)
                .is_none()
        })
    {
        return Err(ConversionError::InvalidField {
            protocol,
            field: "usage".to_owned(),
        });
    }
    match protocol {
        WireProtocol::OpenAiChat => crate::responseopenai::encode(events),
        WireProtocol::OpenAiResponses => crate::responseresponses::encode(events),
        WireProtocol::Gemini => crate::responsegoogle::encode(events),
        WireProtocol::Anthropic => crate::responseanthropic::encode(events),
    }
}
