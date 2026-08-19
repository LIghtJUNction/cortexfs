use std::time::{Duration, Instant};

use cortexfs_channels::InboundMessage;
use reqwest::blocking::Client;

use crate::channel::{
    bridge::ChannelProgressSink,
    progress::{append_bounded, fits},
};

use super::{DiscordConfig, api, effect, message};

const MAX_PROGRESS_BYTES: usize = 64 * 1024;
const MAX_MESSAGE_CHARS: usize = 1_900;

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

    fn reaction(&self, emoji: Option<&str>, remove: bool) {
        let Some(emoji) = emoji.filter(|value| !value.is_empty()) else {
            return;
        };
        let _ignored = if remove {
            effect::remove(self.client, self.config, &self.channel, &self.source, emoji)
        } else {
            effect::react(self.client, self.config, &self.channel, &self.source, emoji)
        };
    }

    fn should_edit(&self) -> bool {
        let interval_ready = self
            .config
            .progress
            .edit_interval_ms
            .is_some_and(|value| self.last_edit.elapsed() >= Duration::from_millis(value));
        let chunk_ready = self
            .config
            .progress
            .edit_chunk_bytes
            .is_some_and(|value| self.text.len().saturating_sub(self.last_edit_len) >= value);
        interval_ready || chunk_ready
    }
}
impl ChannelProgressSink for Progress<'_> {
    fn begin(&mut self, _inbound: &InboundMessage) {
        self.reaction(self.config.progress.reaction.as_deref(), false);
        if self.config.progress.typing {
            let _ignored = effect::typing(self.client, self.config, &self.channel);
        }
        self.placeholder = self
            .config
            .progress
            .placeholder
            .as_deref()
            .filter(|text| !text.is_empty())
            .and_then(|text| {
                message::create(self.client, self.config, &self.channel, &self.source, text).ok()
            });
    }

    fn delta(&mut self, text: &str) {
        append_bounded(&mut self.text, text, MAX_PROGRESS_BYTES);
        if self.placeholder.is_some() && fits(&self.text, MAX_MESSAGE_CHARS) && self.should_edit() {
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
        self.reaction(self.config.progress.reaction.as_deref(), true);
    }

    fn error(&mut self, message: &str) {
        let text = self
            .config
            .progress
            .error_prefix
            .as_deref()
            .map_or_else(|| message.to_owned(), |prefix| format!("{prefix}{message}"));
        self.delivered = self.edit(&text);
        if !self.delivered {
            self.remove_placeholder();
            let _ignored = api::send_reply(self.client, self.config, &self.channel, &text);
        }
        self.reaction(self.config.progress.reaction.as_deref(), true);
        self.reaction(self.config.progress.error_reaction.as_deref(), false);
    }

    fn completed(&self) -> bool {
        self.delivered
    }
}
