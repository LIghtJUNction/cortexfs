use serde::{Deserialize, Serialize};

/// Fine-grained effects that a channel may perform without sending a message.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelAction {
    Typing,
    Preview,
    Reaction,
    Edit,
    Delete,
    MarkRead,
    Pin,
    Unpin,
    Redact,
}

/// Optional actions advertised in addition to base message capabilities.
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent effect flags are the stable serialized wire shape"
)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelActions {
    pub typing: bool,
    pub preview: bool,
    pub reaction: bool,
    pub edit: bool,
    pub delete: bool,
    pub mark_read: bool,
    pub pin: bool,
    pub unpin: bool,
    pub redact: bool,
}

impl ChannelActions {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            typing: false,
            preview: false,
            reaction: false,
            edit: false,
            delete: false,
            mark_read: false,
            pin: false,
            unpin: false,
            redact: false,
        }
    }

    #[must_use]
    pub const fn supports(self, action: ChannelAction) -> bool {
        match action {
            ChannelAction::Typing => self.typing,
            ChannelAction::Preview => self.preview,
            ChannelAction::Reaction => self.reaction,
            ChannelAction::Edit => self.edit,
            ChannelAction::Delete => self.delete,
            ChannelAction::MarkRead => self.mark_read,
            ChannelAction::Pin => self.pin,
            ChannelAction::Unpin => self.unpin,
            ChannelAction::Redact => self.redact,
        }
    }
}
