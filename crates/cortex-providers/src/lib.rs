#![forbid(unsafe_code)]

use cortex_core::{ApiFormat, ModelId, ProviderId};
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const HTTP_IO_TIMEOUT: Duration = Duration::from_secs(30);

/// Provider health state exposed under `provider/<id>/health`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProviderHealth {
    status: ProviderStatus,
    latency_ms: Option<u64>,
    last_error: Option<String>,
}

impl ProviderHealth {
    /// Create a provider health record.
    #[must_use]
    pub const fn new(
        status: ProviderStatus,
        latency_ms: Option<u64>,
        last_error: Option<String>,
    ) -> Self {
        Self {
            status,
            latency_ms,
            last_error,
        }
    }

    /// Healthy provider record.
    #[must_use]
    pub const fn healthy() -> Self {
        Self::new(ProviderStatus::Ready, None, None)
    }

    /// Current provider status.
    #[must_use]
    pub const fn status(&self) -> ProviderStatus {
        self.status
    }

    /// Last measured latency in milliseconds.
    #[must_use]
    pub const fn latency_ms(&self) -> Option<u64> {
        self.latency_ms
    }

    /// Last provider error, if any.
    #[must_use]
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}

/// Provider health status.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ProviderStatus {
    /// Provider is ready.
    Ready,
    /// Provider is configured but currently degraded.
    Degraded,
    /// Provider is disabled by policy or config.
    Disabled,
    /// Provider is not configured.
    MissingConfiguration,
    /// Last health check failed.
    Failed,
}

impl ProviderStatus {
    /// Return the status text used by FUSE attribute files.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Disabled => "disabled",
            Self::MissingConfiguration => "missing_configuration",
            Self::Failed => "failed",
        }
    }
}

/// Model exposed by a provider.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProviderModel {
    id: ModelId,
    format: ApiFormat,
    context_window: Option<u64>,
    max_output_tokens: Option<u64>,
    capabilities: Vec<String>,
}

impl ProviderModel {
    /// Create a provider model descriptor.
    #[must_use]
    pub const fn new(id: ModelId, format: ApiFormat) -> Self {
        Self {
            id,
            format,
            context_window: None,
            max_output_tokens: None,
            capabilities: Vec::new(),
        }
    }

    /// Set the context window.
    #[must_use]
    pub const fn with_context_window(mut self, value: u64) -> Self {
        self.context_window = Some(value);
        self
    }

    /// Set the maximum output token count.
    #[must_use]
    pub const fn with_max_output_tokens(mut self, value: u64) -> Self {
        self.max_output_tokens = Some(value);
        self
    }

    /// Set model capabilities.
    #[must_use]
    pub fn with_capabilities(mut self, values: Vec<String>) -> Self {
        self.capabilities = values;
        self
    }

    /// Model id.
    #[must_use]
    pub const fn id(&self) -> &ModelId {
        &self.id
    }

    /// Native request/response format used by the model.
    #[must_use]
    pub const fn format(&self) -> ApiFormat {
        self.format
    }

    /// Context window, if known.
    #[must_use]
    pub const fn context_window(&self) -> Option<u64> {
        self.context_window
    }

    /// Maximum output token count, if known.
    #[must_use]
    pub const fn max_output_tokens(&self) -> Option<u64> {
        self.max_output_tokens
    }

    /// Model capability strings.
    #[must_use]
    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }
}

/// Native provider request.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProviderRequest {
    format: ApiFormat,
    provider: Option<ProviderId>,
    model: Option<ModelId>,
    body: String,
}

impl ProviderRequest {
    /// Create a native provider request.
    #[must_use]
    pub fn new(format: ApiFormat, body: impl Into<String>) -> Self {
        Self {
            format,
            provider: None,
            model: None,
            body: body.into(),
        }
    }

    /// Set the selected provider instance.
    #[must_use]
    pub fn with_provider(mut self, provider: ProviderId) -> Self {
        self.provider = Some(provider);
        self
    }

    /// Set the provider-local model target.
    #[must_use]
    pub fn with_model(mut self, model: ModelId) -> Self {
        self.model = Some(model);
        self
    }

    /// Native API format.
    #[must_use]
    pub const fn format(&self) -> ApiFormat {
        self.format
    }

    /// Selected provider instance, if routing already chose one.
    #[must_use]
    pub const fn provider(&self) -> Option<&ProviderId> {
        self.provider.as_ref()
    }

    /// Provider-local model target, if routing already selected one.
    #[must_use]
    pub const fn model(&self) -> Option<&ModelId> {
        self.model.as_ref()
    }

    /// Raw native JSON request body.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }
}

/// Native provider response.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProviderResponse {
    format: ApiFormat,
    body: String,
}

impl ProviderResponse {
    /// Create a native provider response.
    #[must_use]
    pub fn new(format: ApiFormat, body: impl Into<String>) -> Self {
        Self {
            format,
            body: body.into(),
        }
    }

    /// Native API format.
    #[must_use]
    pub const fn format(&self) -> ApiFormat {
        self.format
    }

    /// Raw native JSON response body.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }
}

/// Provider adapter failure.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ProviderError {
    /// Requested format is not supported by this provider.
    UnsupportedFormat(ApiFormat),
    /// Requested model is not exposed by this provider for the selected format.
    UnsupportedModel {
        /// Provider-local model identity.
        model: ModelId,
        /// Requested API format.
        format: ApiFormat,
    },
    /// Provider has no response placeholder configured.
    MissingResponse,
    /// Provider transport failed.
    Transport(String),
    /// Provider returned an invalid response body.
    InvalidResponse(String),
}

impl Display for ProviderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::UnsupportedFormat(format) => {
                write!(f, "unsupported provider format: {format:?}")
            }
            Self::UnsupportedModel { ref model, format } => {
                write!(f, "unsupported provider model: {model} for {format}")
            }
            Self::MissingResponse => f.write_str("missing provider response"),
            Self::Transport(ref message) => write!(f, "provider transport failed: {message}"),
            Self::InvalidResponse(ref message) => write!(f, "invalid provider response: {message}"),
        }
    }
}

impl Error for ProviderError {}

/// Provider result.
pub type ProviderResult<T> = Result<T, ProviderError>;

/// Provider adapter boundary used by `cortexd`.
pub trait Provider: Debug + Send + Sync {
    /// Provider identity.
    fn id(&self) -> &ProviderId;

    /// Formats supported by this provider instance.
    fn formats(&self) -> &[ApiFormat];

    /// Current provider health.
    fn health(&self) -> ProviderHealth;

    /// Discover or return known models.
    fn models(&self) -> Vec<ProviderModel>;

    /// Execute a native provider request.
    fn call(&self, request: ProviderRequest) -> ProviderResult<ProviderResponse>;
}

impl<T> Provider for Box<T>
where
    T: Provider + ?Sized,
{
    fn id(&self) -> &ProviderId {
        self.as_ref().id()
    }

    fn formats(&self) -> &[ApiFormat] {
        self.as_ref().formats()
    }

    fn health(&self) -> ProviderHealth {
        self.as_ref().health()
    }

    fn models(&self) -> Vec<ProviderModel> {
        self.as_ref().models()
    }

    fn call(&self, request: ProviderRequest) -> ProviderResult<ProviderResponse> {
        self.as_ref().call(request)
    }
}

/// Local Ollama provider adapter.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct OllamaProvider {
    id: ProviderId,
    base_url: String,
    model: ModelId,
    formats: Vec<ApiFormat>,
}

impl OllamaProvider {
    /// Create an Ollama adapter.
    #[must_use]
    pub fn new(id: ProviderId, base_url: impl Into<String>, model: ModelId) -> Self {
        Self {
            id,
            base_url: base_url.into(),
            model,
            formats: vec![ApiFormat::OpenAiChat],
        }
    }

    /// Default local Ollama adapter used by integration tests.
    ///
    /// # Errors
    ///
    /// Returns a validation error if the built-in provider or model id is not
    /// valid for the filesystem ABI.
    pub fn local_smollm2() -> Result<Self, cortex_core::ValidationError> {
        Ok(Self::new(
            ProviderId::new("ollama")?,
            "http://127.0.0.1:11434",
            ModelId::new("smollm2:135m")?,
        ))
    }

    fn call_ollama(&self, request: &ProviderRequest) -> ProviderResult<ProviderResponse> {
        if request.format() != ApiFormat::OpenAiChat {
            return Err(ProviderError::UnsupportedFormat(request.format()));
        }

        let target_model = request.model().unwrap_or(&self.model);
        if target_model != &self.model {
            return Err(ProviderError::UnsupportedModel {
                model: target_model.clone(),
                format: request.format(),
            });
        }

        let request_body = parse_json(request.body())?;
        let messages = request_body
            .get("messages")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        let ollama_body = serde_json::json!({
            "model": self.model.as_str(),
            "stream": false,
            "messages": messages,
            "options": {
                "temperature": 0
            }
        })
        .to_string();
        let body = post_json(&self.base_url, "/api/chat", &ollama_body)
            .and_then(|body| parse_json(&body))?;
        let content = body
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ProviderError::InvalidResponse("missing message.content".to_owned()))?;
        let openai_response = serde_json::json!({
            "id": "chatcmpl-ollama",
            "object": "chat.completion",
            "model": self.model.as_str(),
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": content
                    },
                    "finish_reason": "stop"
                }
            ]
        });
        Ok(ProviderResponse::new(
            ApiFormat::OpenAiChat,
            openai_response.to_string(),
        ))
    }
}

impl Provider for OllamaProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn formats(&self) -> &[ApiFormat] {
        &self.formats
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::healthy()
    }

    fn models(&self) -> Vec<ProviderModel> {
        vec![
            ProviderModel::new(self.model.clone(), ApiFormat::OpenAiChat)
                .with_capabilities(vec!["chat".to_owned(), "local".to_owned()]),
        ]
    }

    fn call(&self, request: ProviderRequest) -> ProviderResult<ProviderResponse> {
        self.call_ollama(&request)
    }
}

fn parse_json(body: &str) -> ProviderResult<serde_json::Value> {
    serde_json::from_str(body).map_err(|error| ProviderError::InvalidResponse(error.to_string()))
}

fn post_json(base_url: &str, path: &str, body: &str) -> ProviderResult<String> {
    let endpoint = HttpEndpoint::parse(base_url)?;
    let mut stream = endpoint.connect()?;
    stream
        .set_read_timeout(Some(HTTP_IO_TIMEOUT))
        .map_err(|error| ProviderError::Transport(error.to_string()))?;
    stream
        .set_write_timeout(Some(HTTP_IO_TIMEOUT))
        .map_err(|error| ProviderError::Transport(error.to_string()))?;
    let request_target = endpoint.request_target(path);
    let host = endpoint.host_header();
    let request = format!(
        "POST {request_target} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nAccept: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| ProviderError::Transport(error.to_string()))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| ProviderError::Transport(error.to_string()))?;
    parse_http_response(&response)
}

fn parse_http_response(response: &str) -> ProviderResult<String> {
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| ProviderError::InvalidResponse("missing http body".to_owned()))?;
    let status = head
        .lines()
        .next()
        .ok_or_else(|| ProviderError::InvalidResponse("missing http status".to_owned()))?;
    if !status.contains(" 200 ") {
        return Err(ProviderError::Transport(status.to_owned()));
    }
    Ok(body.to_owned())
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct HttpEndpoint {
    host: String,
    port: u16,
    path_prefix: String,
}

impl HttpEndpoint {
    fn parse(base_url: &str) -> ProviderResult<Self> {
        let stripped = base_url.strip_prefix("http://").ok_or_else(|| {
            ProviderError::Transport("only http:// base_url is supported".to_owned())
        })?;
        let (authority, path_prefix) = stripped
            .split_once('/')
            .map_or((stripped, ""), |parts| parts);
        let (host, port) = authority
            .split_once(':')
            .map_or_else(|| Ok((authority.to_owned(), 80)), parse_host_port)?;
        if host.is_empty() {
            return Err(ProviderError::Transport("empty http host".to_owned()));
        }
        Ok(Self {
            host,
            port,
            path_prefix: path_prefix.trim_end_matches('/').to_owned(),
        })
    }

    fn connect(&self) -> ProviderResult<TcpStream> {
        let mut last_error = None;
        for address in self.addresses()? {
            match TcpStream::connect_timeout(&address, HTTP_CONNECT_TIMEOUT) {
                Ok(stream) => return Ok(stream),
                Err(error) => last_error = Some(format!("{address}: {error}")),
            }
        }
        Err(ProviderError::Transport(
            last_error.unwrap_or_else(|| "connect failed".to_owned()),
        ))
    }

    fn request_target(&self, path: &str) -> String {
        if self.path_prefix.is_empty() {
            return path.to_owned();
        }
        format!("/{}{}", self.path_prefix, path)
    }

    fn host_header(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    fn addresses(&self) -> ProviderResult<Vec<SocketAddr>> {
        let addresses = (self.host.as_str(), self.port)
            .to_socket_addrs()
            .map_err(|error| ProviderError::Transport(error.to_string()))?
            .collect::<Vec<_>>();
        if addresses.is_empty() {
            return Err(ProviderError::Transport(
                "host resolved to no addresses".to_owned(),
            ));
        }
        Ok(addresses)
    }
}

fn parse_host_port((host, port): (&str, &str)) -> ProviderResult<(String, u16)> {
    let port = port
        .parse::<u16>()
        .map_err(|error| ProviderError::Transport(error.to_string()))?;
    Ok((host.to_owned(), port))
}

/// In-memory provider placeholder for routing and filesystem integration tests.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InMemoryProvider {
    id: ProviderId,
    formats: Vec<ApiFormat>,
    models: Vec<ProviderModel>,
    health: ProviderHealth,
    responses: Vec<CannedResponse>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct CannedResponse {
    provider: Option<ProviderId>,
    format: ApiFormat,
    model: Option<ModelId>,
    response: ProviderResponse,
}

impl InMemoryProvider {
    /// Create an in-memory provider with supported formats.
    #[must_use]
    pub fn new(id: ProviderId, formats: Vec<ApiFormat>) -> Self {
        Self {
            id,
            formats,
            models: Vec::new(),
            health: ProviderHealth::healthy(),
            responses: Vec::new(),
        }
    }

    /// Replace known models.
    #[must_use]
    pub fn with_models(mut self, models: Vec<ProviderModel>) -> Self {
        self.models = models;
        self
    }

    /// Replace health state.
    #[must_use]
    pub fn with_health(mut self, health: ProviderHealth) -> Self {
        self.health = health;
        self
    }

    /// Set a canned response for a format.
    #[must_use]
    pub fn with_response(mut self, format: ApiFormat, response: ProviderResponse) -> Self {
        self = self.remove_response(None, format, None);
        self.responses.push(CannedResponse {
            provider: None,
            format,
            model: None,
            response,
        });
        self
    }

    /// Set a canned response for a selected provider and format.
    #[must_use]
    pub fn with_provider_response(
        mut self,
        provider: ProviderId,
        format: ApiFormat,
        response: ProviderResponse,
    ) -> Self {
        self = self.remove_response(Some(&provider), format, None);
        self.responses.push(CannedResponse {
            provider: Some(provider),
            format,
            model: None,
            response,
        });
        self
    }

    /// Set a canned response for a format and model pair.
    #[must_use]
    pub fn with_model_response(
        mut self,
        format: ApiFormat,
        model: ModelId,
        response: ProviderResponse,
    ) -> Self {
        self = self.remove_response(None, format, Some(&model));
        self.responses.push(CannedResponse {
            provider: None,
            format,
            model: Some(model),
            response,
        });
        self
    }

    fn remove_response(
        mut self,
        provider: Option<&ProviderId>,
        format: ApiFormat,
        model: Option<&ModelId>,
    ) -> Self {
        self.responses.retain(|response_entry| {
            response_entry.provider.as_ref() != provider
                || response_entry.format != format
                || response_entry.model.as_ref() != model
        });
        self
    }
}

impl Provider for InMemoryProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn formats(&self) -> &[ApiFormat] {
        &self.formats
    }

    fn health(&self) -> ProviderHealth {
        self.health.clone()
    }

    fn models(&self) -> Vec<ProviderModel> {
        self.models.clone()
    }

    fn call(&self, request: ProviderRequest) -> ProviderResult<ProviderResponse> {
        if !self.formats.contains(&request.format()) {
            return Err(ProviderError::UnsupportedFormat(request.format()));
        }

        if let Some(model) = request.model()
            && !self
                .models
                .iter()
                .any(|candidate| candidate.format() == request.format() && candidate.id() == model)
        {
            return Err(ProviderError::UnsupportedModel {
                model: model.clone(),
                format: request.format(),
            });
        }

        for response_entry in &self.responses {
            if response_entry.provider.as_ref() == request.provider()
                && response_entry.format == request.format()
                && response_entry.model.as_ref() == request.model()
            {
                return Ok(response_entry.response.clone());
            }
        }

        for response_entry in &self.responses {
            if response_entry.provider.as_ref() == request.provider()
                && response_entry.format == request.format()
                && response_entry.model.is_none()
            {
                return Ok(response_entry.response.clone());
            }
        }

        for response_entry in &self.responses {
            if response_entry.provider.is_none()
                && response_entry.format == request.format()
                && response_entry.model.as_ref() == request.model()
            {
                return Ok(response_entry.response.clone());
            }
        }

        for response_entry in &self.responses {
            if response_entry.provider.is_none()
                && response_entry.format == request.format()
                && response_entry.model.is_none()
            {
                return Ok(response_entry.response.clone());
            }
        }
        Err(ProviderError::MissingResponse)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        InMemoryProvider, OllamaProvider, Provider, ProviderError, ProviderModel, ProviderRequest,
        ProviderResponse,
    };
    use cortex_core::{ApiFormat, ModelId, ProviderId, ValidationError};

    fn provider_id() -> Result<ProviderId, ValidationError> {
        ProviderId::new("moonshot")
    }

    fn model_id(value: &str) -> Result<ModelId, ValidationError> {
        ModelId::new(value)
    }

    #[test]
    fn model_descriptor_keeps_stable_model_identity_and_format() -> Result<(), ValidationError> {
        let model = ProviderModel::new(model_id("kimi-k2")?, ApiFormat::OpenAiChat)
            .with_context_window(131_072)
            .with_max_output_tokens(16_384)
            .with_capabilities(vec!["chat".to_owned(), "tool_call".to_owned()]);

        assert_eq!(model.id().as_str(), "kimi-k2");
        assert_eq!(model.format(), ApiFormat::OpenAiChat);
        assert_eq!(model.context_window(), Some(131_072));
        assert_eq!(model.max_output_tokens(), Some(16_384));
        assert_eq!(
            model.capabilities(),
            ["chat".to_owned(), "tool_call".to_owned()]
        );
        Ok(())
    }

    #[test]
    fn request_can_target_format_and_optional_provider_model() -> Result<(), ValidationError> {
        let relay = ProviderId::new("relay-openai")?;
        let request = ProviderRequest::new(ApiFormat::AnthropicMessages, "{}")
            .with_provider(relay)
            .with_model(model_id("claude")?);

        assert_eq!(request.format(), ApiFormat::AnthropicMessages);
        assert_eq!(
            request.provider().map(ProviderId::as_str),
            Some("relay-openai")
        );
        assert_eq!(request.model().map(ModelId::as_str), Some("claude"));
        assert_eq!(request.body(), "{}");
        Ok(())
    }

    #[test]
    fn ollama_provider_exposes_local_smollm2_model() -> Result<(), Box<dyn std::error::Error>> {
        let provider = OllamaProvider::local_smollm2()?;
        let models = provider.models();
        let model = models.first().ok_or("missing smollm2 model")?;

        assert_eq!(provider.id().as_str(), "ollama");
        assert_eq!(provider.formats(), &[ApiFormat::OpenAiChat]);
        assert_eq!(models.len(), 1);
        assert_eq!(model.id().as_str(), "smollm2:135m");
        assert_eq!(model.format(), ApiFormat::OpenAiChat);
        assert_eq!(
            model.capabilities(),
            ["chat".to_owned(), "local".to_owned()]
        );
        Ok(())
    }

    #[test]
    fn ollama_provider_rejects_non_chat_format() -> Result<(), Box<dyn std::error::Error>> {
        let provider = OllamaProvider::local_smollm2()?;
        let error = provider
            .call(ProviderRequest::new(ApiFormat::OpenAiResponses, "{}"))
            .map_err(|error| error.to_string());

        assert_eq!(
            error,
            Err("unsupported provider format: OpenAiResponses".to_owned())
        );
        Ok(())
    }

    #[test]
    fn ollama_provider_rejects_unknown_model_without_network()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = OllamaProvider::local_smollm2()?;
        let error = provider
            .call(
                ProviderRequest::new(ApiFormat::OpenAiChat, r#"{"messages":[]}"#)
                    .with_model(model_id("not-smollm2")?),
            )
            .map_err(|error| error.to_string());

        assert_eq!(
            error,
            Err("unsupported provider model: not-smollm2 for openai.chat".to_owned())
        );
        Ok(())
    }

    #[test]
    fn in_memory_provider_routes_by_format_and_model() -> Result<(), Box<dyn std::error::Error>> {
        let kimi = model_id("kimi-k2")?;
        let provider = InMemoryProvider::new(provider_id()?, vec![ApiFormat::OpenAiChat])
            .with_models(vec![ProviderModel::new(
                kimi.clone(),
                ApiFormat::OpenAiChat,
            )])
            .with_response(
                ApiFormat::OpenAiChat,
                ProviderResponse::new(ApiFormat::OpenAiChat, r#"{"fallback":true}"#),
            )
            .with_model_response(
                ApiFormat::OpenAiChat,
                kimi.clone(),
                ProviderResponse::new(ApiFormat::OpenAiChat, r#"{"model":"kimi-k2"}"#),
            );

        let response =
            provider.call(ProviderRequest::new(ApiFormat::OpenAiChat, "{}").with_model(kimi))?;

        assert_eq!(response.format(), ApiFormat::OpenAiChat);
        assert_eq!(response.body(), r#"{"model":"kimi-k2"}"#);
        Ok(())
    }

    #[test]
    fn in_memory_provider_falls_back_to_format_response_without_model_target()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = InMemoryProvider::new(provider_id()?, vec![ApiFormat::OpenAiChat])
            .with_response(
                ApiFormat::OpenAiChat,
                ProviderResponse::new(ApiFormat::OpenAiChat, r#"{"ok":true}"#),
            );

        let response = provider.call(ProviderRequest::new(ApiFormat::OpenAiChat, "{}"))?;

        assert_eq!(response.body(), r#"{"ok":true}"#);
        Ok(())
    }

    #[test]
    fn in_memory_provider_routes_canned_response_by_selected_provider()
    -> Result<(), Box<dyn std::error::Error>> {
        let relay = ProviderId::new("relay-openai")?;
        let provider = InMemoryProvider::new(provider_id()?, vec![ApiFormat::OpenAiResponses])
            .with_response(
                ApiFormat::OpenAiResponses,
                ProviderResponse::new(ApiFormat::OpenAiResponses, r#"{"provider":"default"}"#),
            )
            .with_provider_response(
                relay.clone(),
                ApiFormat::OpenAiResponses,
                ProviderResponse::new(ApiFormat::OpenAiResponses, r#"{"provider":"relay"}"#),
            );

        let response = provider.call(
            ProviderRequest::new(ApiFormat::OpenAiResponses, r#"{"input":"hello"}"#)
                .with_provider(relay),
        )?;

        assert_eq!(response.body(), r#"{"provider":"relay"}"#);
        Ok(())
    }

    #[test]
    fn in_memory_provider_rejects_unsupported_format() -> Result<(), ValidationError> {
        let provider = InMemoryProvider::new(provider_id()?, vec![ApiFormat::OpenAiChat]);

        let error = provider
            .call(ProviderRequest::new(ApiFormat::GoogleGenerateContent, "{}"))
            .map_err(|error| error.to_string());

        assert_eq!(
            error,
            Err("unsupported provider format: GoogleGenerateContent".to_owned())
        );
        Ok(())
    }

    #[test]
    fn in_memory_provider_rejects_unknown_model_for_selected_format() -> Result<(), ValidationError>
    {
        let unknown = model_id("unknown-model")?;
        let provider =
            InMemoryProvider::new(provider_id()?, vec![ApiFormat::OpenAiChat]).with_models(vec![
                ProviderModel::new(model_id("kimi-k2")?, ApiFormat::OpenAiChat),
            ]);

        let error = provider
            .call(ProviderRequest::new(ApiFormat::OpenAiChat, "{}").with_model(unknown.clone()));

        assert_eq!(
            error,
            Err(ProviderError::UnsupportedModel {
                model: unknown,
                format: ApiFormat::OpenAiChat,
            })
        );
        Ok(())
    }
}
