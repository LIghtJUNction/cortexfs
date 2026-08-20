use crate::{MetadataSource, Modality, ModelStatus, ReasoningMetadata, Support};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Metadata for one canonical provider/model identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModelMetadata {
    pub provider: String,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub knowledge: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub last_updated: Option<String>,
    pub aliases: Vec<String>,
    pub status: ModelStatus,
    pub context_window_tokens: Option<u32>,
    #[serde(default)]
    pub recommended_context_tokens: Option<u32>,
    #[serde(default)]
    pub compaction_threshold_tokens: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub input_modalities: Vec<Modality>,
    pub output_modalities: Vec<Modality>,
    pub tools: Support,
    pub structured_output: Support,
    pub streaming: Support,
    pub reasoning: ReasoningMetadata,
    #[serde(default)]
    pub attachment: Support,
    #[serde(default)]
    pub temperature: Support,
    #[serde(default)]
    pub open_weights: Support,
    #[serde(default)]
    pub interleaved: Support,
    /// Exact model.dev model object retained for forward-compatible exposure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models_dev: Option<Value>,
    /// Exact provider-independent model object from models.dev/catalog.json.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models_dev_base: Option<Value>,
    pub sources: Vec<MetadataSource>,
}

impl ModelMetadata {
    /// Starts a custom object with conservative unknown capabilities.
    #[must_use]
    pub fn new(
        provider: impl Into<String>,
        id: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            id: id.into(),
            name: name.into(),
            description: None,
            family: None,
            knowledge: None,
            release_date: None,
            last_updated: None,
            aliases: Vec::new(),
            status: ModelStatus::Active,
            context_window_tokens: None,
            recommended_context_tokens: None,
            compaction_threshold_tokens: None,
            max_output_tokens: None,
            input_modalities: vec![Modality::Text],
            output_modalities: vec![Modality::Text],
            tools: Support::Unknown,
            structured_output: Support::Unknown,
            streaming: Support::Unknown,
            reasoning: ReasoningMetadata::default(),
            attachment: Support::Unknown,
            temperature: Support::Unknown,
            open_weights: Support::Unknown,
            interleaved: Support::Unknown,
            models_dev: None,
            models_dev_base: None,
            sources: Vec::new(),
        }
    }

    /// Adds aliases which resolve to this canonical model.
    #[must_use]
    pub fn with_aliases<I, S>(mut self, aliases: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.aliases.extend(aliases.into_iter().map(Into::into));
        self
    }

    /// Sets the context window in tokens.
    #[must_use]
    pub const fn with_context(mut self, tokens: u32) -> Self {
        self.context_window_tokens = Some(tokens);
        self.recommended_context_tokens = Some(crate::recommended_context_tokens(tokens));
        self.compaction_threshold_tokens = Some(crate::compaction_threshold_tokens(
            crate::recommended_context_tokens(tokens),
        ));
        self
    }

    /// Sets a bounded working-window recommendation and compaction trigger.
    #[must_use]
    pub fn with_context_policy(mut self, recommended: u32, compact: u32) -> Self {
        self.recommended_context_tokens =
            Some(recommended.min(self.context_window_tokens.unwrap_or(recommended)));
        self.compaction_threshold_tokens = Some(compact.min(recommended).max(1));
        self
    }

    /// Sets the maximum output size in tokens.
    #[must_use]
    pub const fn with_max_output(mut self, tokens: u32) -> Self {
        self.max_output_tokens = Some(tokens);
        self
    }

    /// Sets input and output modalities.
    #[must_use]
    pub fn with_modalities(mut self, input: &[Modality], output: &[Modality]) -> Self {
        self.input_modalities = input.to_vec();
        self.output_modalities = output.to_vec();
        self
    }

    /// Sets normalized capability flags.
    #[must_use]
    pub const fn with_capabilities(
        mut self,
        tools: Support,
        structured_output: Support,
        streaming: Support,
    ) -> Self {
        self.tools = tools;
        self.structured_output = structured_output;
        self.streaming = streaming;
        self
    }

    /// Sets reasoning levels and the provider control parameter.
    #[must_use]
    pub fn with_reasoning(mut self, levels: &[&str], parameter: impl Into<String>) -> Self {
        self.reasoning = ReasoningMetadata {
            support: Support::Supported,
            levels: levels.iter().map(|level| (*level).to_owned()).collect(),
            parameter: Some(parameter.into()),
            default_level: None,
            max_tokens: None,
        };
        self
    }
    /// Records one provenance URL for this fact set.
    #[must_use]
    pub fn with_source(mut self, source: MetadataSource) -> Self {
        self.sources.push(source);
        self
    }

    /// Retains the exact official model.dev object for consumers and ABI files.
    #[must_use]
    pub fn with_models_dev(mut self, value: Value) -> Self {
        self.models_dev = Some(value);
        self
    }

    /// Retains the exact provider-independent models.dev model object.
    #[must_use]
    pub fn with_models_dev_base(mut self, value: Value) -> Self {
        self.models_dev_base = Some(value);
        self
    }

    /// Tests whether an input modality is explicitly supported.
    #[must_use]
    pub fn supports_input(&self, modality: Modality) -> bool {
        self.input_modalities.contains(&modality)
    }
}
