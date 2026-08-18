use cortexfs_channels::{
    ChannelActions, ChannelCapabilities, ChannelFrame, ChannelFrameBody, ChannelId,
};

#[derive(Clone, Copy)]
pub(super) struct DriverFeatures {
    capabilities: ChannelCapabilities,
    actions: ChannelActions,
    negotiated: bool,
}

impl Default for DriverFeatures {
    fn default() -> Self {
        Self {
            capabilities: ChannelCapabilities::text(),
            actions: all_actions(),
            negotiated: false,
        }
    }
}

impl DriverFeatures {
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "matching the borrowed handshake keeps capability negotiation allocation-free"
    )]
    pub(super) fn observe(&mut self, frame: &ChannelFrame, channel_id: &ChannelId) {
        if let ChannelFrameBody::Hello {
            channel,
            capabilities,
            actions,
            ..
        } = &frame.frame
            && channel == channel_id
        {
            self.capabilities = *capabilities;
            self.actions = *actions;
            self.negotiated = true;
        }
    }

    pub(super) const fn actions(self) -> ChannelActions {
        self.actions
    }

    pub(super) const fn commands_enabled(self) -> bool {
        !self.negotiated || self.capabilities.commands
    }
}

const fn all_actions() -> ChannelActions {
    ChannelActions {
        typing: true,
        preview: true,
        reaction: true,
        edit: true,
        delete: true,
        mark_read: true,
        pin: true,
        unpin: true,
        redact: true,
    }
}

#[cfg(test)]
mod tests {
    use super::DriverFeatures;
    use cortexfs_channels::{
        ChannelActions, ChannelCapabilities, ChannelFrame, ChannelFrameBody, ChannelId,
    };

    #[test]
    fn hello_negotiates_effects_and_commands() {
        let channel = ChannelId::from_static("test");
        let mut features = DriverFeatures::default();
        assert!(features.commands_enabled());
        assert!(
            features
                .actions()
                .supports(cortexfs_channels::ChannelAction::Typing)
        );
        features.observe(
            &ChannelFrame::new(ChannelFrameBody::Hello {
                request_id: "hello".to_owned(),
                channel: channel.clone(),
                capabilities: ChannelCapabilities::text(),
                actions: ChannelActions::empty(),
            }),
            &channel,
        );
        assert!(!features.commands_enabled());
        assert!(
            !features
                .actions()
                .supports(cortexfs_channels::ChannelAction::Typing)
        );
    }
}
