use cortexfs_channels::ChannelDriverSession;

use crate::{
    config::Config,
    error::Result,
    http::WebhookEvent,
    provider::{self, Calls},
};

pub(super) fn handle(
    config: &Config,
    session: &ChannelDriverSession,
    calls: &mut Calls,
    event: &WebhookEvent,
) -> Result<()> {
    if let Some(message) = provider::incoming(config, &event.content_type, &event.body, calls)? {
        session.send_inbound(message)?;
    }
    Ok(())
}
