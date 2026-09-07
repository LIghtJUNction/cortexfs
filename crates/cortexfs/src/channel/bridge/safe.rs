use super::ChannelBridgeError;
use cortexfs_channels::{ChannelError, OutboundMessage};

impl ChannelBridgeError {
    /// Consume a sender denial without delivery; preserve every other failure.
    pub fn consume_denied(
        result: Result<OutboundMessage, Self>,
    ) -> Result<Option<OutboundMessage>, Self> {
        match result {
            Ok(outbound) => Ok(Some(outbound)),
            Err(Self::Channel(ChannelError::SenderDenied)) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

pub(super) fn message(error: &ChannelBridgeError) -> &'static str {
    match *error {
        ChannelBridgeError::Channel(_) => {
            "The channel could not process this message; please try again."
        }
        ChannelBridgeError::Runtime(_) => {
            "The agent runtime is temporarily unavailable; please try again later."
        }
        ChannelBridgeError::EmptyReply => {
            "The model returned no displayable reply; please try again."
        }
        ChannelBridgeError::Agent(_) => {
            "The agent model/tool loop failed; inspect run diagnostics before retrying."
        }
    }
}
