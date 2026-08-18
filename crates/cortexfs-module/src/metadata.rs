use crate::valid_name;

/// Versioned identifier for the static Rust module API.
pub const CORTEX_MODULE_ABI: &str = "cortexfs.module/v1";

use serde::{Deserialize, Serialize};

/// Runtime extension domains sharing the module lifecycle.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleKind {
    /// Executable or hosted Agent behavior.
    Agent,
    /// Capability endpoint invoked by the runtime.
    Tool,
    /// External communication driver.
    Channel,
    /// Provider-neutral inference adapter.
    Model,
    /// Context compiler or pass.
    Context,
}

/// A provider-neutral capability advertised by a module.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ModuleCapability {
    /// Stable capability name, not a provider-specific API label.
    pub name: String,
    /// Optional human-readable capability description.
    pub description: String,
}

/// Immutable identity and capability declaration for one module.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModuleMetadata {
    /// Stable object-like module id.
    pub id: String,
    /// Module ABI implementation version.
    pub version: String,
    /// Extension domain.
    pub kind: ModuleKind,
    /// Declared provider-neutral capabilities.
    pub capabilities: Vec<ModuleCapability>,
}

impl ModuleMetadata {
    /// Creates metadata for a module implementation.
    #[must_use]
    pub fn new(id: impl Into<String>, version: impl Into<String>, kind: ModuleKind) -> Self {
        Self {
            id: id.into(),
            version: version.into(),
            kind,
            capabilities: Vec::new(),
        }
    }

    /// Adds one capability while constructing metadata.
    #[must_use]
    pub fn with_capability(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.capabilities.push(ModuleCapability {
            name: name.into(),
            description: description.into(),
        });
        self
    }

    /// Returns whether metadata fits the stable module identifier grammar.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        valid_name(&self.id)
            && !self.version.trim().is_empty()
            && self.capabilities.iter().all(|capability| {
                valid_name(&capability.name) && !capability.description.contains('\0')
            })
    }
}
