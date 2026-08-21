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
        ChannelBridgeError::Agent(_) => "模型服务拒绝请求；请检查账户额度、模型配置和上游可用性。",
    }
}
