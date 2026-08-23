pub const MAX_SKILL_METADATA_CHARS: usize = 8_000;
pub const MAX_HISTORY_MESSAGES_CHARS: usize = cortexfs_context::DEFAULT_HISTORY_CHARS;
pub(crate) const MAX_AGENT_RULES_CHARS: usize = 64_000;
pub(crate) const MAX_AGENT_RULE_FILE_BYTES: u64 = 64 * 1024;
pub(crate) const MAX_SKILL_FILE_BYTES: u64 = 16 * 1024;
pub(crate) const MAX_SKILL_FILES: usize = 256;
pub(crate) const MAX_HISTORY_MESSAGES_READ_BYTES: usize = 64 * 1024;

pub mod compact;
pub mod history;
pub mod model;
pub mod read;
pub mod render;
pub mod rules;
pub mod skills;
pub mod snapshot;

pub(crate) use history::collect_history_messages_for_agent;
pub use history::*;
pub use model::*;
pub(crate) use read::*;
pub use render::*;
pub use rules::*;
pub use skills::*;
pub use snapshot::*;
