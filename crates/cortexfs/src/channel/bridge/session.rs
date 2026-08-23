use cortexfs_channels::InboundMessage;

use super::AgentChannelBridge;

impl AgentChannelBridge {
    pub(super) fn session_for_inbound(&self, inbound: &InboundMessage) -> String {
        let base = self.route.session_for_message(inbound);
        match self.generation(&base) {
            0 => base,
            n => format!("{base}-{n}"),
        }
    }

    pub(super) fn rotate_session(&self, inbound: &InboundMessage) -> String {
        let base = self.route.session_for_message(inbound);
        let next = self.generation(&base).saturating_add(1);
        if let Ok(mut map) = self.generations.lock() {
            map.insert(base.clone(), next);
        }
        format!("started a new session: {base}-{next}")
    }

    fn generation(&self, base: &str) -> u32 {
        self.generations
            .lock()
            .ok()
            .and_then(|map| map.get(base).copied())
            .unwrap_or(0)
    }
}
