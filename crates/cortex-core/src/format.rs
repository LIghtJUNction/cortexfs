use crate::ValidationError;
use core::fmt;
use core::str::FromStr;

/// Stable API request/response formats exposed by `CortexFS`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ApiFormat {
    OpenAiChat,
    OpenAiResponses,
    AnthropicMessages,
    GoogleGenerateContent,
}

impl ApiFormat {
    /// Returns the stable filesystem ABI name for this API format.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiChat => "openai.chat",
            Self::OpenAiResponses => "openai.responses",
            Self::AnthropicMessages => "anthropic.messages",
            Self::GoogleGenerateContent => "google.generate_content",
        }
    }
}

impl fmt::Display for ApiFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ApiFormat {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "openai.chat" => Ok(Self::OpenAiChat),
            "openai.responses" => Ok(Self::OpenAiResponses),
            "anthropic.messages" => Ok(Self::AnthropicMessages),
            "google.generate_content" => Ok(Self::GoogleGenerateContent),
            _ => Err(ValidationError::unsupported_api_format(value)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ApiFormat;
    use crate::ValidationReason;
    use core::str::FromStr;

    #[test]
    fn api_format_round_trips_stable_names() {
        assert_eq!(ApiFormat::OpenAiChat.as_str(), "openai.chat");
        assert_eq!(
            ApiFormat::from_str("google.generate_content"),
            Ok(ApiFormat::GoogleGenerateContent)
        );
    }

    #[test]
    fn unknown_api_format_is_rejected() {
        assert_eq!(
            ApiFormat::from_str("openai-chat").map_err(|error| error.reason()),
            Err(ValidationReason::UnsupportedValue)
        );
    }
}
