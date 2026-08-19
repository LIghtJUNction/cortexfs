use std::{collections::HashMap, num::NonZeroU32};

use crate::abi::constants::{FORBIDDEN_MODEL_CAPABILITIES, STABLE_MODEL_CAPABILITIES};
use crate::support::control::{parse_canonical_control_value, parse_canonical_positive_u32};

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
    /// Capability word is not in the stable semantic capability set.
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
    /// Model can consume video input.
    VideoInput,
    /// Model can produce video output.
    VideoOutput,
    /// Model can consume audio input.
    AudioInput,
    /// Model can produce audio output.
    AudioOutput,
    /// Model can consume PDF input.
    PdfInput,
    /// Model can produce PDF output.
    PdfOutput,
    /// Model accepts file attachments.
    Attachment,
    /// Model accepts temperature control.
    Temperature,
    /// Model exposes interleaved reasoning content.
    Interleaved,
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
    pub video_input: bool,
    pub video_output: bool,
    pub audio_input: bool,
    pub audio_output: bool,
    pub pdf_input: bool,
    pub pdf_output: bool,
    pub attachment: bool,
    pub temperature: bool,
    pub interleaved: bool,
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

/// Trusted hard context limit projected for one model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelContextLimit {
    /// No trusted maximum is available.
    Unknown,
    /// Known positive hard limit in tokens.
    Known(NonZeroU32),
}

impl ModelContextLimit {
    /// Constructs a known limit from a positive token count.
    #[must_use]
    pub const fn known(tokens: u32) -> Option<Self> {
        match NonZeroU32::new(tokens) {
            Some(tokens) => Some(Self::Known(tokens)),
            None => None,
        }
    }

    /// Parses an exact `model/<provider>/<model>.d/limit` file body.
    #[must_use]
    pub fn parse_control(content: &str) -> Option<Self> {
        let value = parse_canonical_control_value(content)?;
        if value == "unknown" {
            return Some(Self::Unknown);
        }
        NonZeroU32::new(parse_canonical_positive_u32(value)?).map(Self::Known)
    }

    /// Returns the known token maximum, if the catalog or host supplied one.
    #[must_use]
    pub const fn tokens(self) -> Option<u32> {
        match self {
            Self::Unknown => None,
            Self::Known(tokens) => Some(tokens.get()),
        }
    }
}

impl std::fmt::Display for ModelContextLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Unknown => f.write_str("unknown"),
            Self::Known(tokens) => tokens.fmt(f),
        }
    }
}

/// Supported provider-neutral model reasoning effort levels for control files.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ModelEffort {
    /// Use provider/implementation default.
    Auto,
    /// Disable optional reasoning effort.
    None,
    /// Use low effort.
    Low,
    /// Use medium effort.
    Medium,
    /// Use high effort.
    High,
    /// Use extra-high effort.
    XHigh,
    /// Use maximum effort when the provider supports it.
    Max,
}

impl std::fmt::Display for ModelEffort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Auto => write!(f, "auto"),
            Self::None => write!(f, "none"),
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
            Self::XHigh => write!(f, "xhigh"),
            Self::Max => write!(f, "max"),
        }
    }
}

impl std::str::FromStr for ModelEffort {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value {
            "auto" => Self::Auto,
            "none" => Self::None,
            "low" => Self::Low,
            "medium" => Self::Medium,
            "high" => Self::High,
            "xhigh" => Self::XHigh,
            "max" => Self::Max,
            _ => return Err("unsupported effort"),
        })
    }
}

impl ModelEffort {
    /// Parses effort from ABI control text, defaulting empty input to auto.
    #[must_use]
    pub fn parse(content: &str) -> Option<Self> {
        let value = content.trim();
        if value.is_empty() {
            return Some(Self::Auto);
        }
        value.parse::<Self>().ok()
    }

    /// Parses effort from a `.d/effort` line.
    #[must_use]
    pub fn parse_line(content: &str) -> Option<Self> {
        Self::parse(content)
    }

    /// Canonical ABI file body value for this effort.
    #[must_use]
    pub fn as_control_value(self) -> &'static str {
        match self {
            Self::Auto => "auto\n",
            Self::None => "none\n",
            Self::Low => "low\n",
            Self::Medium => "medium\n",
            Self::High => "high\n",
            Self::XHigh => "xhigh\n",
            Self::Max => "max\n",
        }
    }
}

pub mod routes;
pub use routes::*;

impl_issue_report!(ModelCapabilityReport, ModelCapabilityIssue);

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
            Capability::VideoInput => self.video_input,
            Capability::VideoOutput => self.video_output,
            Capability::AudioInput => self.audio_input,
            Capability::AudioOutput => self.audio_output,
            Capability::PdfInput => self.pdf_input,
            Capability::PdfOutput => self.pdf_output,
            Capability::Attachment => self.attachment,
            Capability::Temperature => self.temperature,
            Capability::Interleaved => self.interleaved,
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

/// Inspects a `model/<provider>/<model>.d/cap` file body for stable capability words.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_context_limit_accepts_unknown_or_a_positive_canonical_token_count() {
        assert_eq!(
            ModelContextLimit::parse_control("unknown\n"),
            Some(ModelContextLimit::Unknown)
        );
        assert_eq!(
            ModelContextLimit::parse_control("272000\n").and_then(ModelContextLimit::tokens),
            Some(272_000)
        );
    }

    #[test]
    fn model_context_limit_rejects_noncanonical_or_out_of_range_values() {
        for invalid in [
            "",
            "0\n",
            "-1\n",
            "+1\n",
            "01\n",
            " 1\n",
            "1 \n",
            "1.0\n",
            "4294967296\n",
            "unknown",
            "unknown\nextra\n",
        ] {
            assert_eq!(
                ModelContextLimit::parse_control(invalid),
                None,
                "accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn model_effort_accepts_known_values() {
        assert_eq!(ModelEffort::parse("auto"), Some(ModelEffort::Auto));
        assert_eq!(ModelEffort::parse("none"), Some(ModelEffort::None));
        assert_eq!(ModelEffort::parse("low"), Some(ModelEffort::Low));
        assert_eq!(ModelEffort::parse("medium"), Some(ModelEffort::Medium));
        assert_eq!(ModelEffort::parse("high"), Some(ModelEffort::High));
        assert_eq!(ModelEffort::parse("xhigh"), Some(ModelEffort::XHigh));
        assert_eq!(ModelEffort::parse("max"), Some(ModelEffort::Max));
        assert_eq!(ModelEffort::parse(""), Some(ModelEffort::Auto));
        assert_eq!(ModelEffort::parse("bad"), None);
    }

    #[test]
    fn model_capabilities_parse_trims_stability_words() {
        let report = inspect_model_capabilities("chat\nstream\n");
        assert!(report.is_ok());
    }
}
