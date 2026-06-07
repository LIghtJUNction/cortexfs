use crate::{DEFAULT_BATCH_FORMAT, LOCAL_USER_ID};

pub const API_FORMATS: [&str; 4] = [
    "openai.chat",
    "openai.responses",
    "anthropic.messages",
    "google.generate_content",
];

const LOCAL_RUNTIME_BASE_URL_TEXT: &str = "http://127.0.0.1:6186\n";
const LOCAL_RELAY_BASE_URL_TEXT: &str = "http://127.0.0.1:6187/v1\n";
const NO_DEFAULT_PROVIDER_ID: &str = "";

pub const PROVIDER_SPECS: &[ProviderRuntimeSpec] = &[
    ProviderRuntimeSpec {
        id: "openai-main",
        family: "openai\n",
        name: "OpenAI primary API account\n",
        formats: &["openai.chat", "openai.responses"],
        default_base_url: "https://api.openai.com/v1\n",
        auth_scheme: "bearer\n",
        account_type: "api_key\n",
        priority: "80\n",
        secret_status: "missing\n",
        default_model: "gpt-4.1-mini",
        context_window: "1047576\n",
        max_output_tokens: "32768\n",
        model_capabilities: "chat\nresponses\ncloud\n",
    },
    ProviderRuntimeSpec {
        id: "anthropic-main",
        family: "anthropic\n",
        name: "Anthropic primary API account\n",
        formats: &["anthropic.messages"],
        default_base_url: "https://api.anthropic.com\n",
        auth_scheme: "x-api-key\n",
        account_type: "api_key\n",
        priority: "70\n",
        secret_status: "missing\n",
        default_model: "claude-3-5-haiku-latest",
        context_window: "200000\n",
        max_output_tokens: "8192\n",
        model_capabilities: "chat\ncloud\n",
    },
    ProviderRuntimeSpec {
        id: "google-main",
        family: "google\n",
        name: "Google Gemini primary API account\n",
        formats: &["google.generate_content"],
        default_base_url: "https://generativelanguage.googleapis.com\n",
        auth_scheme: "api_key\n",
        account_type: "api_key\n",
        priority: "70\n",
        secret_status: "missing\n",
        default_model: "gemini-2.0-flash",
        context_window: "1048576\n",
        max_output_tokens: "8192\n",
        model_capabilities: "chat\ncloud\n",
    },
    ProviderRuntimeSpec {
        id: "relay-openai",
        family: "relay\n",
        name: "Relay endpoint using OpenAI formats\n",
        formats: &["openai.chat", "openai.responses"],
        default_base_url: "https://relay.example.invalid/v1\n",
        auth_scheme: "bearer\n",
        account_type: "api_key\n",
        priority: "60\n",
        secret_status: "missing\n",
        default_model: "default",
        context_window: "\n",
        max_output_tokens: "\n",
        model_capabilities: "chat\nresponses\nrelay\n",
    },
    ProviderRuntimeSpec {
        id: "kimi-main",
        family: "moonshot\n",
        name: "Kimi API account\n",
        formats: &["openai.chat"],
        default_base_url: "https://api.moonshot.cn/v1\n",
        auth_scheme: "bearer\n",
        account_type: "api_key\n",
        priority: "55\n",
        secret_status: "missing\n",
        default_model: "kimi-k2-0711-preview",
        context_window: "131072\n",
        max_output_tokens: "16384\n",
        model_capabilities: "chat\ncloud\n",
    },
    ProviderRuntimeSpec {
        id: "minimax-main",
        family: "minimax\n",
        name: "MiniMax API account\n",
        formats: &["openai.chat"],
        default_base_url: "https://api.minimax.chat/v1\n",
        auth_scheme: "bearer\n",
        account_type: "api_key\n",
        priority: "55\n",
        secret_status: "missing\n",
        default_model: "MiniMax-M1",
        context_window: "1000000\n",
        max_output_tokens: "80000\n",
        model_capabilities: "chat\ncloud\n",
    },
    ProviderRuntimeSpec {
        id: "local-runtime",
        family: "local-runtime\n",
        name: "Local runtime provider\n",
        formats: &["openai.chat"],
        default_base_url: LOCAL_RUNTIME_BASE_URL_TEXT,
        auth_scheme: "none\n",
        account_type: "local_runtime\n",
        priority: "50\n",
        secret_status: "not_required\n",
        default_model: "cortexfs-test-model",
        context_window: "8192\n",
        max_output_tokens: "1024\n",
        model_capabilities: "chat\nlocal\n",
    },
    ProviderRuntimeSpec {
        id: "local-relay",
        family: "local-relay\n",
        name: "Local relay provider\n",
        formats: &["openai.chat"],
        default_base_url: LOCAL_RELAY_BASE_URL_TEXT,
        auth_scheme: "none\n",
        account_type: "local_runtime\n",
        priority: "49\n",
        secret_status: "not_required\n",
        default_model: "cortexfs-local-relay-model",
        context_window: "8192\n",
        max_output_tokens: "1024\n",
        model_capabilities: "chat\nlocal\n",
    },
];

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ProviderRuntimeSpec {
    pub id: &'static str,
    pub family: &'static str,
    pub name: &'static str,
    pub formats: &'static [&'static str],
    pub default_base_url: &'static str,
    pub auth_scheme: &'static str,
    pub account_type: &'static str,
    pub priority: &'static str,
    pub secret_status: &'static str,
    pub default_model: &'static str,
    pub context_window: &'static str,
    pub max_output_tokens: &'static str,
    pub model_capabilities: &'static str,
}

pub fn configured_provider_ids() -> impl Iterator<Item = &'static str> {
    PROVIDER_SPECS.iter().map(|provider| provider.id)
}

pub fn provider_spec(provider: &str) -> Option<ProviderRuntimeSpec> {
    PROVIDER_SPECS
        .iter()
        .copied()
        .find(|spec| spec.id == provider)
}

pub fn provider_supports_format(provider: &ProviderRuntimeSpec, format: &str) -> bool {
    provider.formats.contains(&format)
}

pub fn default_model_for_provider(provider: &str) -> Option<&'static str> {
    provider_spec(provider).map(|spec| spec.default_model)
}

pub fn default_provider_id() -> &'static str {
    PROVIDER_SPECS
        .first()
        .map_or(NO_DEFAULT_PROVIDER_ID, |provider| provider.id)
}

pub fn global_model_count() -> String {
    format!("{}\n", PROVIDER_SPECS.len())
}

pub fn global_model_list() -> String {
    newline_list(PROVIDER_SPECS.iter().map(provider_model_id))
}

pub fn provider_count() -> String {
    format!("{}\n", PROVIDER_SPECS.len())
}

pub fn provider_list() -> String {
    newline_list(configured_provider_ids())
}

pub fn provider_count_for_format(format: &str) -> String {
    format!("{}\n", providers_for_format(format).count())
}

pub fn provider_list_for_format(format: &str) -> String {
    newline_list(providers_for_format(format).map(|provider| provider.id))
}

pub fn model_count_for_format(format: &str) -> String {
    format!("{}\n", providers_for_format(format).count())
}

pub fn model_list_for_format(format: &str) -> String {
    newline_list(providers_for_format(format).map(provider_model_id))
}

pub fn providers_for_format(
    format: &str,
) -> impl Iterator<Item = &'static ProviderRuntimeSpec> + '_ {
    PROVIDER_SPECS
        .iter()
        .filter(move |provider| provider.formats.contains(&format))
}

pub fn provider_model_id(provider: &ProviderRuntimeSpec) -> String {
    format!("{}.{}", provider.id, provider.default_model)
}

pub fn provider_child_path(provider: &str, child: &str) -> Vec<String> {
    vec!["provider".to_owned(), provider.to_owned(), child.to_owned()]
}

pub fn user_model_path(provider: &ProviderRuntimeSpec) -> Vec<String> {
    vec![
        "home".to_owned(),
        LOCAL_USER_ID.to_owned(),
        "model".to_owned(),
        provider_model_id(provider),
    ]
}

pub fn default_format(provider: &ProviderRuntimeSpec) -> &'static str {
    provider
        .formats
        .first()
        .copied()
        .unwrap_or(DEFAULT_BATCH_FORMAT)
}

pub fn in_memory_execution_provider_spec() -> Option<ProviderRuntimeSpec> {
    PROVIDER_SPECS
        .iter()
        .copied()
        .find(|provider| provider.account_type.trim() == "local_runtime")
        .or_else(|| PROVIDER_SPECS.first().copied())
}

pub fn newline_list(items: impl Iterator<Item = impl AsRef<str>>) -> String {
    let mut content = String::new();
    for item in items {
        content.push_str(item.as_ref());
        content.push('\n');
    }
    content
}

pub fn provider_response_for_format(provider: &ProviderRuntimeSpec, format: &str) -> String {
    if format == "openai.chat" {
        provider_chat_response(provider.id, provider.default_model)
    } else {
        provider_format_response(provider.id, format)
    }
}

pub fn provider_chat_response(provider: &str, model: &str) -> String {
    format!(
        r#"{{"id":"chatcmpl-cortexfs","object":"chat.completion","provider":"{provider}","model":"{model}","choices":[{{"index":0,"message":{{"role":"assistant","content":"cortexfs-ok"}},"finish_reason":"stop"}}]}}"#
    )
}

pub fn provider_format_response(provider: &str, format: &str) -> String {
    format!(r#"{{"status":"accepted","provider":"{provider}","format":"{format}"}}"#)
}

#[cfg(test)]
pub fn default_provider_spec() -> fuse3::Result<ProviderRuntimeSpec> {
    provider_spec(default_provider_id()).ok_or_else(fuse3::Errno::new_not_exist)
}

#[cfg(test)]
pub fn local_execution_provider_spec() -> fuse3::Result<ProviderRuntimeSpec> {
    in_memory_execution_provider_spec().ok_or_else(fuse3::Errno::new_not_exist)
}

#[cfg(test)]
pub fn alternate_provider_for_format(
    provider: &ProviderRuntimeSpec,
    format: &str,
) -> fuse3::Result<ProviderRuntimeSpec> {
    PROVIDER_SPECS
        .iter()
        .copied()
        .find(|candidate| {
            candidate.id != provider.id && provider_supports_format(candidate, format)
        })
        .ok_or_else(fuse3::Errno::new_not_exist)
}

#[cfg(test)]
pub fn alternate_provider_spec(
    provider: &ProviderRuntimeSpec,
) -> fuse3::Result<ProviderRuntimeSpec> {
    alternate_provider_for_format(provider, DEFAULT_BATCH_FORMAT)
}

#[cfg(test)]
pub fn ensure_provider(provider: &str) -> fuse3::Result<()> {
    provider_spec(provider)
        .map(|_spec| ())
        .ok_or_else(|| fuse3::Errno::from(libc::EINVAL))
}

#[cfg(test)]
pub fn invalid_provider_id() -> &'static str {
    "missing-provider"
}
