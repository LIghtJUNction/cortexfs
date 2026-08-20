use cortexfs_channels::{
    ChannelActions, ChannelCapabilities, ChannelCommand, ChannelCommandResult, ChannelEffect,
    ChannelError, ChannelHealth, ChannelId, ChannelRuntimeEvent, DeliveryReceipt, MessageTarget,
    OutboundMessage,
};

/// Platform behavior hosted by [`crate::ChannelRuntime`].
pub trait ChannelService: Send {
    fn id(&self) -> ChannelId;
    fn capabilities(&self) -> ChannelCapabilities;

    fn actions(&self) -> ChannelActions {
        ChannelActions::empty()
    }

    fn start(&mut self) -> Result<(), ChannelError> {
        Ok(())
    }

    fn outbound(&mut self, message: &OutboundMessage) -> Result<DeliveryReceipt, ChannelError>;

    fn effect(
        &mut self,
        _target: &MessageTarget,
        _effect: &ChannelEffect,
    ) -> Result<(), ChannelError> {
        Err(ChannelError::Unsupported(
            "channel does not apply live effects".to_owned(),
        ))
    }

    fn command(
        &mut self,
        _session: &str,
        _command_id: &str,
        _command: &ChannelCommand,
        _target: Option<&MessageTarget>,
    ) -> Result<ChannelCommandResult, ChannelError> {
        Ok(ChannelCommandResult::Rejected {
            reason: "channel does not accept runtime commands".to_owned(),
        })
    }

    fn health(&mut self) -> Result<ChannelHealth, ChannelError> {
        Ok(ChannelHealth::ready())
    }

    fn runtime_event(&mut self, _event: ChannelRuntimeEvent) -> Result<(), ChannelError> {
        Ok(())
    }

    fn stop(&mut self) -> Result<(), ChannelError> {
        Ok(())
    }
}
