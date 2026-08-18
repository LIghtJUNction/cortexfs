use cortexfs_channels::{ChannelCodec, ChannelEffect, InboundMessage, MessageTarget};
use reqwest::blocking::Client;

use cortexfs::channel::bridge::ChannelProgressSink;

use super::{WebhookConfig, outbound};

pub(super) struct Progress<'a> {
    client: &'a Client,
    config: &'a WebhookConfig,
    codec: &'a dyn ChannelCodec,
    target: MessageTarget,
    active: bool,
}

impl<'a> Progress<'a> {
    pub(super) fn new(
        client: &'a Client,
        config: &'a WebhookConfig,
        codec: &'a dyn ChannelCodec,
        target: MessageTarget,
    ) -> Self {
        Self {
            client,
            config,
            codec,
            target,
            active: false,
        }
    }

    fn emit(&self, effect: &ChannelEffect) {
        let Ok(Some(request)) = self.codec.encode_effect(&self.target, effect) else {
            return;
        };
        let _ignored = outbound::send(self.client, self.config, request);
    }

    fn start(&mut self) {
        if !self.active {
            self.active = true;
            self.emit(&ChannelEffect::Typing { active: true });
        }
    }
}

impl ChannelProgressSink for Progress<'_> {
    fn begin(&mut self, _inbound: &InboundMessage) {
        self.start();
    }

    fn begin_event(&mut self, _target: &MessageTarget) {
        self.start();
    }

    fn complete(&mut self, _text: &str) {
        if self.active {
            self.emit(&ChannelEffect::Typing { active: false });
            self.active = false;
        }
    }

    fn error(&mut self, _message: &str) {
        self.complete("");
    }
}
