use crate::{MetadataSource, Modality, ModelStatus, ReasoningMetadata, Support};
use serde::{Deserialize, Serialize};

/// Metadata for one canonical provider/model identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModelMetadata {
    pub provider: String,
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub status: ModelStatus,
    pub context_window_tokens: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub input_modalities: Vec<Modality>,
    pub output_modalities: Vec<Modality>,
    pub tools: Support,
    pub structured_output: Support,
    pub streaming: Support,
    pub reasoning: ReasoningMetadata,
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
            aliases: Vec::new(),
            status: ModelStatus::Active,
            context_window_tokens: None,
            max_output_tokens: None,
            input_modalities: vec![Modality::Text],
            output_modalities: vec![Modality::Text],
            tools: Support::Unknown,
            structured_output: Support::Unknown,
            streaming: Support::Unknown,
            reasoning: ReasoningMetadata::default(),
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

    /// Tests whether an input modality is explicitly supported.
    #[must_use]
    pub fn supports_input(&self, modality: Modality) -> bool {
        self.input_modalities.contains(&modality)
    }
}
