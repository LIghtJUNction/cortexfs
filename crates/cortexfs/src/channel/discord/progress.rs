use std::time::{Duration, Instant};

use cortexfs_channels::InboundMessage;
use reqwest::blocking::Client;

use crate::channel::{
    bridge::ChannelProgressSink,
    progress::{append_bounded, fits},
};

use super::{DiscordConfig, api, effect, message};

const EYES: &str = "%F0%9F%91%80";
const CROSS: &str = "%E2%9D%8C";
const MAX_PROGRESS_BYTES: usize = 64 * 1024;
const MAX_MESSAGE_CHARS: usize = 1_900;
const EDIT_INTERVAL: Duration = Duration::from_millis(700);

pub(super) struct Progress<'a> {
    client: &'a Client,
    config: &'a DiscordConfig,
    channel: String,
    source: String,
    placeholder: Option<String>,
    text: String,
    last_edit: Instant,
    last_edit_len: usize,
    delivered: bool,
}

impl<'a> Progress<'a> {
    pub(super) fn new(
        client: &'a Client,
        config: &'a DiscordConfig,
        inbound: &InboundMessage,
    ) -> Self {
        Self {
            client,
            config,
            channel: inbound.target.conversation.to_string(),
            source: inbound.id.clone(),
            placeholder: None,
            text: String::new(),
            last_edit: Instant::now(),
            last_edit_len: 0,
            delivered: false,
        }
    }

    fn edit(&mut self, text: &str) -> bool {
        let Some(message) = self.placeholder.as_deref() else {
            return false;
        };
        if message::edit(self.client, self.config, &self.channel, message, text).is_ok() {
            self.last_edit = Instant::now();
            self.last_edit_len = self.text.len();
            self.delivered = true;
            true
        } else {
            false
        }
    }

    fn remove_placeholder(&mut self) {
        if let Some(message) = self.placeholder.take() {
            let _ignored = message::delete(self.client, self.config, &self.channel, &message);
        }
    }

    fn cleanup_reaction(&self, emoji: &str) {
        let _ignored = effect::remove(self.client, self.config, &self.channel, &self.source, emoji);
    }
}
impl ChannelProgressSink for Progress<'_> {
    fn begin(&mut self, _inbound: &InboundMessage) {
        let _ignored = effect::react(self.client, self.config, &self.channel, &self.source, EYES);
        let _ignored = effect::typing(self.client, self.config, &self.channel);
        self.placeholder =
            message::create(self.client, self.config, &self.channel, &self.source).ok();
    }

    fn delta(&mut self, text: &str) {
        append_bounded(&mut self.text, text, MAX_PROGRESS_BYTES);
        if self.placeholder.is_some()
            && fits(&self.text, MAX_MESSAGE_CHARS)
            && (self.last_edit.elapsed() >= EDIT_INTERVAL
                || self.text.len().saturating_sub(self.last_edit_len) >= 512)
        {
            let text = self.text.clone();
            self.edit(&text);
        }
    }

    fn complete(&mut self, text: &str) {
        self.delivered = false;
        if fits(text, MAX_MESSAGE_CHARS) {
            if !self.edit(text) {
                self.remove_placeholder();
            }
        } else {
            self.remove_placeholder();
        }
        self.cleanup_reaction(EYES);
    }

    fn error(&mut self, message: &str) {
        let text = format!("⚠️ {message}");
        self.delivered = self.edit(&text);
        if !self.delivered {
            self.remove_placeholder();
            let _ignored = api::send_reply(self.client, self.config, &self.channel, &text);
        }
        self.cleanup_reaction(EYES);
        let _ignored = effect::react(self.client, self.config, &self.channel, &self.source, CROSS);
    }

    fn completed(&self) -> bool {
        self.delivered
    }
}
