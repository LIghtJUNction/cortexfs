use std::time::{Duration, Instant};

use cortexfs_channels::InboundMessage;
use reqwest::blocking::Client;

use crate::channel::{
    bridge::ChannelProgressSink,
    progress::{append_bounded, fits},
};

use super::{TelegramConfig, message, request};

const MAX_BYTES: usize = 64 * 1024;
const MAX_CHARS: usize = 4_000;

pub(super) struct Progress<'a> {
    client: &'a Client,
    config: &'a TelegramConfig,
    chat: String,
    source: String,
    thread: Option<String>,
    placeholder: Option<String>,
    text: String,
    last_edit: Instant,
    last_edit_len: usize,
    delivered: bool,
}

impl<'a> Progress<'a> {
    pub(super) fn new(
        client: &'a Client,
        config: &'a TelegramConfig,
        inbound: &InboundMessage,
    ) -> Self {
        Self {
            client,
            config,
            chat: inbound.target.conversation.to_string(),
            source: inbound.id.clone(),
            thread: inbound.target.thread.clone(),
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
        if message::edit(self.client, self.config, &self.chat, message, text).is_ok() {
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
            let _ignored = request::delete(self.client, self.config, &self.chat, &message);
        }
    }

    fn react(&self, emoji: Option<&str>) {
        if let Some(emoji) = emoji.filter(|value| !value.is_empty()) {
            let _ignored = message::react(
                self.client,
                self.config,
                &self.chat,
                &self.source,
                Some(emoji),
            );
        }
    }

    fn clear_reaction(&self) {
        if self
            .config
            .progress
            .reaction
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        {
            let _ignored = message::react(self.client, self.config, &self.chat, &self.source, None);
        }
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
        self.react(self.config.progress.reaction.as_deref());
        if self.config.progress.typing {
            let _ignored = message::typing(self.client, self.config, &self.chat);
        }
        self.placeholder = self
            .config
            .progress
            .placeholder
            .as_deref()
            .filter(|text| !text.is_empty())
            .and_then(|text| {
                message::create(
                    self.client,
                    self.config,
                    &self.chat,
                    &self.source,
                    text,
                    self.thread.as_deref(),
                )
                .ok()
            });
    }
    fn delta(&mut self, text: &str) {
        append_bounded(&mut self.text, text, MAX_BYTES);
        if self.placeholder.is_some() && fits(&self.text, MAX_CHARS) && self.should_edit() {
            let text = self.text.clone();
            self.edit(&text);
        }
    }
    fn complete(&mut self, text: &str) {
        self.delivered = false;
        if fits(text, MAX_CHARS) {
            if !self.edit(text) {
                self.remove_placeholder();
            }
        } else {
            self.remove_placeholder();
            self.delivered = message::send_text(self.client, self.config, &self.chat, text).is_ok();
        }
        self.clear_reaction();
    }
    fn error(&mut self, error: &str) {
        let text = self
            .config
            .progress
            .error_prefix
            .as_deref()
            .map_or_else(|| error.to_owned(), |prefix| format!("{prefix}{error}"));
        self.delivered = self.edit(&text);
        if !self.delivered {
            self.remove_placeholder();
            let _ignored = message::send_text(self.client, self.config, &self.chat, &text);
        }
        self.clear_reaction();
        self.react(self.config.progress.error_reaction.as_deref());
    }
    fn completed(&self) -> bool {
        self.delivered
    }
}
