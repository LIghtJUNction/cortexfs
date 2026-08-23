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

#[cfg(test)]
mod tests {
    use cortexfs_channels::{
        ChannelId, ChannelSessionRoute, ConversationId, InboundMessage, MessageBody, MessageTarget,
        Participant,
    };

    use super::AgentChannelBridge;

    #[test]
    fn slash_new_rotates_the_derived_session() {
        let route = ChannelSessionRoute::new("coder", "im").expect("route");
        let bridge = AgentChannelBridge::new("/tmp/agent.sock", route, None);
        let inbound = InboundMessage {
            id: "1".into(),
            target: MessageTarget {
                channel: ChannelId::new("telegram").expect("channel"),
                conversation: ConversationId::new("dm").expect("conversation"),
                thread: None,
                reply_to: None,
            },
            sender: Participant::default(),
            body: MessageBody::text("/new").expect("body"),
            timestamp_ms: None,
            metadata: Default::default(),
        };
        let first = bridge.session_for_inbound(&inbound);
        assert!(
            bridge
                .rotate_session(&inbound)
                .contains(&format!("{first}-1"))
        );
        assert_eq!(bridge.session_for_inbound(&inbound), format!("{first}-1"));
    }
}
