use std::future::Future;
use std::pin::Pin;

use crate::ModuleMetadata;

/// Result type shared by all module lifecycle operations.
pub type ModuleResult<T> = Result<T, ModuleError>;

/// Executor-independent boxed lifecycle future.
pub type ModuleFuture<'a> = Pin<Box<dyn Future<Output = ModuleResult<()>> + Send + 'a>>;

/// Host context passed to a module without exposing filesystem internals.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleContext {
    /// Runtime instance identifier chosen by the host.
    pub instance: String,
}

/// Coarse lifecycle state owned by a module registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleState {
    Registered,
    Initialized,
    Running,
    Stopped,
    Shutdown,
}

/// Stable module failure with a subsystem-neutral code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModuleError {
    InvalidMetadata,
    Duplicate(String),
    InvalidState,
    Failed {
        code: String,
        message: String,
    },
    Lifecycle {
        module: String,
        operation: &'static str,
        source: Box<Self>,
    },
}

impl std::fmt::Display for ModuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::InvalidMetadata => f.write_str("invalid module metadata"),
            Self::Duplicate(ref id) => write!(f, "duplicate module: {id}"),
            Self::InvalidState => f.write_str("invalid module lifecycle state"),
            Self::Failed {
                ref code,
                ref message,
            } => write!(f, "{code}: {message}"),
            Self::Lifecycle {
                ref module,
                operation,
                ref source,
            } => write!(f, "module {module} {operation} failed: {source}"),
        }
    }
}

impl std::error::Error for ModuleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            Self::Lifecycle { ref source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

/// Runtime-owned extension boundary.
pub trait CortexModule: Send {
    /// Returns immutable module identity and capabilities.
    fn metadata(&self) -> &ModuleMetadata;

    /// Initializes the module against a host instance.
    fn init<'a>(&'a mut self, context: &'a ModuleContext) -> ModuleFuture<'a>;

    /// Starts accepting work.
    fn start(&mut self) -> ModuleFuture<'_>;

    /// Stops accepting new work while preserving durable state.
    fn stop(&mut self) -> ModuleFuture<'_>;

    /// Releases runtime resources.
    fn shutdown(&mut self) -> ModuleFuture<'_>;
}
