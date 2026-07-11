pub const MAX_SKILL_METADATA_CHARS: usize = 32_000;
pub const MAX_HISTORY_MESSAGES_CHARS: usize = 8_000;
pub(crate) const MAX_AGENT_RULES_CHARS: usize = 64_000;
pub(crate) const MAX_AGENT_RULE_FILE_BYTES: u64 = 64 * 1024;
pub(crate) const MAX_SKILL_FILE_BYTES: u64 = 16 * 1024;
pub(crate) const MAX_SKILL_FILES: usize = 256;
pub(crate) const MAX_HISTORY_MESSAGES_READ_BYTES: u64 = 64 * 1024;
pub(crate) const MAX_HISTORY_MESSAGE_LINE_BYTES: usize = 16 * 1024;

pub mod read;
pub use read as file_read;
pub mod history;
pub mod render;
pub use render as prompt_render;
pub mod rules;
pub mod skills;
pub mod snapshot;

pub(crate) use file_read::*;
pub use history::*;
pub use prompt_render::*;
pub use rules::*;
pub use skills::*;
pub use snapshot::*;
