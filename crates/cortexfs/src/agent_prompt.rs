use std::collections::VecDeque;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::DEFAULT_AGENT_PROMPT_TEMPLATE;
use nix::libc;
use serde_json::Value;

pub const MAX_SKILL_METADATA_CHARS: usize = 32_000;
pub const MAX_HISTORY_MESSAGES_CHARS: usize = 8_000;
const MAX_AGENT_RULES_CHARS: usize = 64_000;
const MAX_AGENT_RULE_FILE_BYTES: u64 = 64 * 1024;
const MAX_SKILL_FILE_BYTES: u64 = 16 * 1024;
const MAX_SKILL_FILES: usize = 256;
const MAX_HISTORY_MESSAGES_READ_BYTES: u64 = 64 * 1024;
const MAX_HISTORY_MESSAGE_LINE_BYTES: usize = 16 * 1024;

include!("agent_prompt/prompt_render.rs");
include!("agent_prompt/rules.rs");
include!("agent_prompt/skills.rs");
include!("agent_prompt/file_read.rs");
include!("agent_prompt/history.rs");
