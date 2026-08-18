use cortexfs_channels::{ChannelCodec, ChannelIncoming, platform::mattermost::MattermostCodec};
use reqwest::blocking::Client;

use crate::channel::bridge::AgentChannelBridge;

use super::{MattermostConfig, MattermostError, api};

pub(super) fn handle_event(
    client: &Client,
    config: &MattermostConfig,
    bridge: &AgentChannelBridge,
    user_id: &str,
    payload: &str,
) -> Result<(), MattermostError> {
    let Some(incoming) = MattermostCodec.decode_incoming(payload)? else {
        return Ok(());
    };
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "matching borrowed channel items keeps the authorization check zero-copy"
    )]
    let (target, sender) = match &incoming {
        ChannelIncoming::Message(message) => (&message.target, Some(message.sender.id.as_str())),
        ChannelIncoming::Event(event) => (
            &event.context().target,
            event
                .context()
                .participant
                .as_ref()
                .map(|item| item.id.as_str()),
        ),
    };
    if sender == Some(user_id) || !config.accepts(target.conversation.as_str()) {
        return Ok(());
    }
    let outbound = bridge.handle_incoming(incoming)?;
    api::send(client, config, MattermostCodec.encode(&outbound)?)?;
    Ok(())
}
