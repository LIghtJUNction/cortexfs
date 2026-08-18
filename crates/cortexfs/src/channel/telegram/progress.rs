use std::time::{Duration, Instant};

use cortexfs_channels::InboundMessage;
use reqwest::blocking::Client;

use crate::channel::{
    bridge::ChannelProgressSink,
    progress::{append_bounded, fits},
};

use super::{TelegramConfig, message, request};

const EYES: &str = "👀";
const CROSS: &str = "❌";
const MAX_BYTES: usize = 64 * 1024;
const MAX_CHARS: usize = 4_000;
const EDIT_INTERVAL: Duration = Duration::from_millis(700);

pub(super) struct Progress<'a> {
    client: &'a Client,
    config: &'a TelegramConfig,
    chat: String,
    source: String,
    thread: Option<String>,
    placeholder: Option<String>,
    text: String,
    last_edit: Instant,
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
            delivered: false,
        }
    }
    fn edit(&mut self, text: &str) -> bool {
        let Some(message) = self.placeholder.as_deref() else {
            return false;
        };
        if message::edit(self.client, self.config, &self.chat, message, text).is_ok() {
            self.last_edit = Instant::now();
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
        let _ignored = message::react(self.client, self.config, &self.chat, &self.source, emoji);
    }
}

impl ChannelProgressSink for Progress<'_> {
    fn begin(&mut self, _inbound: &InboundMessage) {
        self.react(Some(EYES));
        let _ignored = message::typing(self.client, self.config, &self.chat);
        self.placeholder = message::create(
            self.client,
            self.config,
            &self.chat,
            &self.source,
            self.thread.as_deref(),
        )
        .ok();
    }
    fn delta(&mut self, text: &str) {
        append_bounded(&mut self.text, text, MAX_BYTES);
        if self.placeholder.is_some()
            && fits(&self.text, MAX_CHARS)
            && (self.last_edit.elapsed() >= EDIT_INTERVAL)
        {
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
        self.react(None);
    }
    fn error(&mut self, error: &str) {
        let text = format!("⚠️ {error}");
        self.delivered = self.edit(&text);
        if !self.delivered {
            self.remove_placeholder();
            let _ignored = message::send_text(self.client, self.config, &self.chat, &text);
        }
        self.react(None);
        self.react(Some(CROSS));
    }
    fn completed(&self) -> bool {
        self.delivered
    }
}
