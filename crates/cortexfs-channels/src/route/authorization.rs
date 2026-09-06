use super::ChannelSessionRoute;
use crate::ChannelError;

impl ChannelSessionRoute {
    /// Replaces the exact external-sender allowlist. Empty lists deny everyone.
    /// IDs come from the authenticated adapter, never message text or display names.
    #[must_use]
    pub fn with_allowed_senders(mut self, senders: impl IntoIterator<Item = String>) -> Self {
        self.allowed_senders = senders
            .into_iter()
            .map(|id| id.trim().to_owned())
            .filter(|id| !id.is_empty() && id != "*")
            .collect();
        self
    }

    /// Checks authority before any session state, progress effect, or agent request.
    pub fn authorize_sender(&self, sender: Option<&str>) -> Result<(), ChannelError> {
        if sender.is_some_and(|id| self.allowed_senders.contains(id)) {
            Ok(())
        } else {
            Err(ChannelError::SenderDenied)
        }
    }
}
