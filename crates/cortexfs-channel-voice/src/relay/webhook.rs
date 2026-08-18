use crate::{
    config::Config,
    error::Result,
    http::WebhookEvent,
    provider::{self, Calls},
    socket::Session,
};

pub(super) fn handle(
    config: &Config,
    session: &Session,
    calls: &mut Calls,
    event: &WebhookEvent,
) -> Result<()> {
    if let Some(message) = provider::incoming(config, &event.content_type, &event.body, calls)? {
        session.send(message)?;
    }
    Ok(())
}
