use super::ChannelBridgeError;

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
