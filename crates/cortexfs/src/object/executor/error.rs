//! Executor-local failures. Display text is part of the runner ABI surface.

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecError {
    message: String,
}

impl ExecError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    #[must_use]
    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn with_io(prefix: &str, error: &std::io::Error) -> Self {
        Self::new(format!("{prefix}: {error}"))
    }
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExecError {}
