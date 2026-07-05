use std::collections::VecDeque;
use std::env;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{DEFAULT_AGENT_PROMPT_TEMPLATE, plain_fs};
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

include!("prompt/prompt_render.rs");
include!("prompt/rules.rs");
include!("prompt/skills.rs");
include!("prompt/file_read.rs");
include!("prompt/history.rs");
