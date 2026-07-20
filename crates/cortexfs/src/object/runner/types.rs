use serde::Deserialize;

pub(crate) const RUNNER_PROVIDER_CONFIG_DIR: &str = cortexfs::SYSTEM_PROVIDER_CONFIG_DIR;
pub(crate) const MAX_RUNNER_PROVIDER_CONFIG_BYTES: u64 = 64 * 1024;
pub(crate) const MAX_RUNTIME_PROVIDER_SECRET_BYTES: u64 = 64 * 1024;
pub(crate) const MAX_PROVIDER_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_PROVIDER_STREAM_LINE_BYTES: usize = 256 * 1024;
pub(crate) const PROVIDER_CURL_BIN: &str = cortexfs::support::command::CURL;
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct RunnerProviderConfig {
    #[serde(default)]
    pub(crate) name: Option<String>,
    pub(crate) base_url: String,
    pub(crate) oauth: Option<cortexfs::OAuthProviderConfig>,
    #[serde(default)]
    pub(crate) formats: Vec<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResolvedTransport {
    Direct {
        base_url: String,
    },
    Http {
        base_url: String,
    },
    Unix {
        base_url: String,
        socket_path: String,
    },
}
pub(crate) struct CurlJsonTarget {
    pub(crate) url: String,
    pub(crate) unix_socket: Option<String>,
}
pub(crate) type CurlJsonOutputParts = (std::process::ExitStatus, Vec<u8>, Vec<u8>);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProviderRuntimeDriver {
    OpenAiChat,
    OpenAiResponses,
    AnthropicMessages,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ProviderCredential {
    Bearer(String),
    Codex { token: String, account_id: String },
    AnthropicApiKey(String),
}
impl ProviderCredential {
    pub(crate) fn secret(&self) -> &str {
        match *self {
            Self::Bearer(ref secret) | Self::AnthropicApiKey(ref secret) => secret,
            Self::Codex { ref token, .. } => token,
        }
    }
    pub(crate) fn codex_account(&self) -> Option<&str> {
        match *self {
            Self::Codex { ref account_id, .. } => Some(account_id),
            Self::Bearer(_) | Self::AnthropicApiKey(_) => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TokenUsage {
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProviderTextCompletion {
    pub(crate) content: String,
    pub(crate) usage: Option<TokenUsage>,
}
