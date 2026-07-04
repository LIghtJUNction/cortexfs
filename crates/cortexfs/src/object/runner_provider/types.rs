const RUNNER_PROVIDER_CONFIG_DIR: &str = "/etc/cortexfs/providers.d";
const MAX_RUNNER_PROVIDER_CONFIG_BYTES: u64 = 64 * 1024;
const MAX_RUNTIME_PROVIDER_SECRET_BYTES: u64 = 64 * 1024;
const MAX_PROVIDER_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_PROVIDER_STREAM_LINE_BYTES: usize = 256 * 1024;
const PROVIDER_CURL_BIN: &str = "/usr/bin/curl";

#[derive(Clone, Debug, Deserialize)]
struct RunnerProviderConfig {
    #[serde(default)]
    name: Option<String>,
    base_url: String,
    oauth: Option<cortexfs::OAuthProviderConfig>,
    #[serde(default)]
    formats: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ResolvedTransport {
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

struct CurlJsonTarget {
    url: String,
    unix_socket: Option<String>,
}

type CurlJsonOutputParts = (std::process::ExitStatus, Vec<u8>, Vec<u8>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProviderRuntimeDriver {
    OpenAiChat,
    OpenAiResponses,
    AnthropicMessages,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProviderCredential {
    Bearer(String),
    AnthropicApiKey(String),
}

impl ProviderCredential {
    fn secret(&self) -> &str {
        match self {
            &Self::Bearer(ref secret) | &Self::AnthropicApiKey(ref secret) => secret,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TokenUsage {
    input_tokens: u64,
    output_tokens: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProviderTextCompletion {
    content: String,
    usage: Option<TokenUsage>,
}
