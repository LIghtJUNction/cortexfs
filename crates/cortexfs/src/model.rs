use std::collections::HashMap;

use crate::abi_constants::{FORBIDDEN_MODEL_CAPABILITIES, STABLE_MODEL_CAPABILITIES};
use crate::abi_path::is_object_name;

/// Model capability control-file validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelCapabilityIssue {
    /// Capability word is provider/API-format private.
    ProviderPrivate {
        /// One-based line number in `cap`.
        line: usize,
        /// Capability word from the file.
        capability: String,
    },
    /// Capability word is not in the stable v1 semantic capability set.
    Unknown {
        /// One-based line number in `cap`.
        line: usize,
        /// Capability word from the file.
        capability: String,
    },
}

/// Result of inspecting `model/<provider>/<model>.d/cap`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModelCapabilityReport {
    issues: Vec<ModelCapabilityIssue>,
}

/// Queryable model capability flag.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Capability {
    /// Model can consume image input.
    Vision,
    /// Model can emit tool-call syntax.
    Tools,
    /// Model supports JSON-mode or structured JSON output.
    JsonMode,
    /// Model can consume image input.
    ImageInput,
    /// Model can produce image output.
    ImageOutput,
    /// Model can consume audio input.
    AudioInput,
    /// Model can produce audio output.
    AudioOutput,
}

/// Provider-neutral model capability declaration.
#[expect(
    clippy::struct_excessive_bools,
    reason = "capability files expose independent stable boolean flags"
)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModelCapabilities {
    pub context_length: usize,
    pub vision: bool,
    pub tools: bool,
    pub json_mode: bool,
    pub image_input: bool,
    pub image_output: bool,
    pub audio_input: bool,
    pub audio_output: bool,
}

/// Provider-neutral model capability lookup table.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModelCapabilityRegistry {
    models: HashMap<String, ModelCapabilities>,
}

/// Error while reading or writing model capability registry data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelRegistryError {
    /// Registry JSON could not be parsed.
    InvalidJson,
    /// Registry JSON has an unexpected shape.
    InvalidShape,
    /// Registry cache could not be read.
    CannotRead,
    /// Registry cache could not be written.
    CannotWrite,
}

/// Model driver call site used to select a driver route.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ModelDriverUseCase {
    /// Fallback route when no use-case-specific route is available.
    Default,
    /// One-shot execution through `model/<provider>/<model>`.
    Exec,
    /// Stateful model socket traffic through `model/<provider>/<model>.sock`.
    Socket,
    /// Agent-owned model calls.
    Agent,
}

/// Error while parsing `model/<provider>/<model>.d/driver`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelDriverRouteError {
    /// The route table has no usable driver declarations.
    Empty,
    /// A route-table line is missing `=`.
    MissingEquals { line: usize },
    /// A route-table key is not one of default, exec, socket, or agent.
    UnknownUseCase { line: usize, value: String },
    /// A route-table key appears more than once.
    DuplicateUseCase { line: usize, value: String },
    /// A driver list is empty or has an empty comma element.
    EmptyDriver { line: usize },
    /// A driver name is not a valid stable component.
    InvalidDriverName { line: usize, value: String },
}

/// Parsed `driver` control-file route table.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModelDriverRoutingTable {
    routes: HashMap<ModelDriverUseCase, Vec<String>>,
}

impl ModelCapabilityReport {
    /// Creates a report with collected model capability issues.
    #[must_use]
    pub const fn new(issues: Vec<ModelCapabilityIssue>) -> Self {
        Self { issues }
    }

    /// Returns true when all capabilities use stable v1 semantic words.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns all detected capability issues.
    #[must_use]
    pub fn issues(&self) -> &[ModelCapabilityIssue] {
        &self.issues
    }
}

impl ModelCapabilities {
    /// Returns whether this declaration supports a capability.
    #[must_use]
    pub const fn supports(&self, capability: Capability) -> bool {
        match capability {
            Capability::Vision => self.vision,
            Capability::Tools => self.tools,
            Capability::JsonMode => self.json_mode,
            Capability::ImageInput => self.image_input,
            Capability::ImageOutput => self.image_output,
            Capability::AudioInput => self.audio_input,
            Capability::AudioOutput => self.audio_output,
        }
    }
}

impl ModelCapabilityRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or replaces one model capability declaration.
    pub fn insert(&mut self, model: String, capabilities: ModelCapabilities) {
        self.models.insert(model, capabilities);
    }

    /// Returns one model capability declaration.
    #[must_use]
    pub fn get(&self, model: &str) -> Option<&ModelCapabilities> {
        self.models.get(model)
    }

    /// Returns whether a model supports a capability.
    #[must_use]
    pub fn supports(&self, model: &str, capability: Capability) -> bool {
        self.get(model)
            .is_some_and(|capabilities| capabilities.supports(capability))
    }

    /// Returns the number of known models.
    #[must_use]
    pub fn len(&self) -> usize {
        self.models.len()
    }

    /// Returns whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }
}

impl ModelDriverUseCase {
    /// Parses one route-table key.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "default" => Some(Self::Default),
            "exec" => Some(Self::Exec),
            "socket" => Some(Self::Socket),
            "agent" => Some(Self::Agent),
            _ => None,
        }
    }

    /// Returns the route-table key.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Exec => "exec",
            Self::Socket => "socket",
            Self::Agent => "agent",
        }
    }
}

impl ModelDriverRoutingTable {
    /// Creates an empty driver routing table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts one ordered route list.
    pub fn insert(&mut self, use_case: ModelDriverUseCase, drivers: Vec<String>) {
        self.routes.insert(use_case, drivers);
    }

    /// Returns the exact route list for one use case.
    #[must_use]
    pub fn get(&self, use_case: ModelDriverUseCase) -> Option<&[String]> {
        self.routes.get(&use_case).map(Vec::as_slice)
    }

    /// Returns the route list for a use case, falling back to `default`.
    #[must_use]
    pub fn drivers_for(&self, use_case: ModelDriverUseCase) -> Option<&[String]> {
        self.get(use_case)
            .or_else(|| self.get(ModelDriverUseCase::Default))
    }

    /// Returns the first selected driver for a use case.
    #[must_use]
    pub fn primary_driver_for(&self, use_case: ModelDriverUseCase) -> Option<&str> {
        self.drivers_for(use_case)
            .and_then(|drivers| drivers.first())
            .map(String::as_str)
    }

    /// Returns whether no route is present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    pub(crate) fn route_value(&self, use_case: ModelDriverUseCase) -> String {
        self.get(use_case)
            .map(|drivers| drivers.join(","))
            .unwrap_or_default()
    }
}

/// Inspects a `model/<provider>/<model>.d/cap` file body for stable v1 capability words.
#[must_use]
pub fn inspect_model_capabilities(content: &str) -> ModelCapabilityReport {
    let mut issues = Vec::new();
    for (index, raw_line) in content.lines().enumerate() {
        let line = index + 1;
        let capability = raw_line.trim();
        if capability.is_empty() {
            continue;
        }
        if FORBIDDEN_MODEL_CAPABILITIES.contains(&capability) {
            issues.push(ModelCapabilityIssue::ProviderPrivate {
                line,
                capability: capability.to_owned(),
            });
        } else if !STABLE_MODEL_CAPABILITIES.contains(&capability) {
            issues.push(ModelCapabilityIssue::Unknown {
                line,
                capability: capability.to_owned(),
            });
        }
    }
    ModelCapabilityReport::new(issues)
}

/// Parses `model/<provider>/<model>.d/driver`.
///
/// A legacy single-line value such as `debug` is treated as `default=debug`.
/// Route-table form supports `default`, `exec`, `socket`, and `agent` keys with
/// comma-separated drivers in priority order.
pub fn parse_model_driver_routes(
    content: &str,
) -> Result<ModelDriverRoutingTable, ModelDriverRouteError> {
    let significant = content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let value = line.trim();
            (!value.is_empty() && !value.starts_with('#')).then_some((index + 1, value))
        })
        .collect::<Vec<_>>();

    if significant.is_empty() {
        return Err(ModelDriverRouteError::Empty);
    }

    if significant.len() == 1 {
        let Some((line, driver)) = significant.first().copied() else {
            return Err(ModelDriverRouteError::Empty);
        };
        if !driver.contains('=') {
            return parse_driver_list(line, driver).map(|drivers| {
                let mut table = ModelDriverRoutingTable::new();
                table.insert(ModelDriverUseCase::Default, drivers);
                table
            });
        }
    }

    let mut table = ModelDriverRoutingTable::new();
    for (line, route) in significant {
        let Some((raw_key, raw_drivers)) = route.split_once('=') else {
            return Err(ModelDriverRouteError::MissingEquals { line });
        };
        let key = raw_key.trim();
        let Some(use_case) = ModelDriverUseCase::parse(key) else {
            return Err(ModelDriverRouteError::UnknownUseCase {
                line,
                value: key.to_owned(),
            });
        };
        if table.get(use_case).is_some() {
            return Err(ModelDriverRouteError::DuplicateUseCase {
                line,
                value: key.to_owned(),
            });
        }
        table.insert(use_case, parse_driver_list(line, raw_drivers)?);
    }

    if table.is_empty() {
        Err(ModelDriverRouteError::Empty)
    } else {
        Ok(table)
    }
}

fn parse_driver_list(line: usize, value: &str) -> Result<Vec<String>, ModelDriverRouteError> {
    let mut drivers = Vec::new();
    for raw_driver in value.split(',') {
        let driver = raw_driver.trim();
        if driver.is_empty() {
            return Err(ModelDriverRouteError::EmptyDriver { line });
        }
        if !is_object_name(driver) {
            return Err(ModelDriverRouteError::InvalidDriverName {
                line,
                value: driver.to_owned(),
            });
        }
        drivers.push(driver.to_owned());
    }
    if drivers.is_empty() {
        Err(ModelDriverRouteError::EmptyDriver { line })
    } else {
        Ok(drivers)
    }
}
