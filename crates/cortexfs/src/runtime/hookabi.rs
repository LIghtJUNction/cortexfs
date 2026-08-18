use std::fmt;

pub const HOOK_ABI: &str = "cortexfs.hook/v1";
pub const MAX_HOOKS: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookPhase {
    Pre,
    Post,
}

impl HookPhase {
    #[must_use]
    pub const fn directory(self) -> &'static str {
        if matches!(self, Self::Pre) {
            "pre.d"
        } else {
            "post.d"
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        if matches!(self, Self::Pre) {
            "pre"
        } else {
            "post"
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HookInvocation<'a> {
    pub phase: HookPhase,
    pub action: &'a str,
    pub agent: &'a str,
    pub run: &'a str,
    pub step: u8,
    pub tool: Option<&'a str>,
    pub status: Option<&'a str>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct HookError {
    code: &'static str,
    hook: String,
}

impl HookError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }
    #[must_use]
    pub fn message(&self) -> String {
        format!("agent hook failed: {}", self.hook)
    }
    pub fn new(code: &'static str, hook: impl Into<String>) -> Self {
        Self {
            code,
            hook: hook.into(),
        }
    }
}

impl fmt::Display for HookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message())
    }
}

impl std::error::Error for HookError {}
