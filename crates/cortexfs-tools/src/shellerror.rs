use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

#[derive(Debug)]
pub enum ShellExecError {
    Spawn(io::Error),
    Wait(io::Error),
    OutputLimit { limit: usize },
    TimedOut { seconds: u64 },
}

impl Display for ShellExecError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Spawn(ref error) => write!(f, "cannot run shell command: {error}"),
            Self::Wait(ref error) => f.write_str(&error.to_string()),
            Self::OutputLimit { limit } => write!(f, "shell command output exceeds {limit} bytes"),
            Self::TimedOut { seconds } => write!(f, "shell command timed out after {seconds}s"),
        }
    }
}

impl ShellExecError {
    #[must_use]
    pub fn contains(&self, text: &str) -> bool {
        self.to_string().contains(text)
    }
}

impl Error for ShellExecError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match *self {
            Self::Spawn(ref error) | Self::Wait(ref error) => Some(error),
            Self::OutputLimit { .. } | Self::TimedOut { .. } => None,
        }
    }
}
