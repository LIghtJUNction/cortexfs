use std::fmt;

pub const COMPACT_ABI: &str = "cortexfs.compact/v1";
pub const MAX_COMPACT_MESSAGES: usize = 64;
pub const MAX_COMPACT_INPUT_BYTES: usize = 64 * 1024;
pub const MAX_COMPACT_OUTPUT_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompactInvocation<'a> {
    pub agent: &'a str,
    pub session: &'a str,
    pub max_chars: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub struct CompactError {
    code: &'static str,
    hook: String,
}

impl CompactError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    #[must_use]
    pub fn message(&self) -> String {
        format!("context compaction failed: {}", self.hook)
    }

    pub fn new(code: &'static str, hook: impl Into<String>) -> Self {
        Self {
            code,
            hook: hook.into(),
        }
    }
}

impl fmt::Display for CompactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message())
    }
}

impl std::error::Error for CompactError {}

pub(crate) fn compact_frame(
    invocation: &CompactInvocation<'_>,
    omitted: &[cortexfs_context::Message],
) -> String {
    let messages = omitted
        .iter()
        .take(MAX_COMPACT_MESSAGES)
        .map(|message| {
            serde_json::json!({
                "role": message.role(),
                "content": message.content().trim(),
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "abi": COMPACT_ABI,
        "agent": invocation.agent,
        "session": invocation.session,
        "max_chars": invocation.max_chars,
        "omitted": omitted.len(),
        "messages": messages,
    })
    .to_string()
}
