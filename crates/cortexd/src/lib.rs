#![forbid(unsafe_code)]

use cortex_core::{ApiFormat, Message, MessageRole, ModelId, ProviderId, ThreadId};
use cortex_providers::{Provider, ProviderError, ProviderRequest};
use cortex_store::{ApiRequest, ApiResponse, AuditEvent, RequestId, Store, StoreError};
use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Native local API endpoint accepted by the daemon front door.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LocalApiEndpoint {
    /// `GET /v1/models`.
    Models,
    /// `POST /v1/chat/completions`.
    ChatCompletions,
    /// `POST /v1/responses`.
    Responses,
    /// `POST /v1/messages`.
    Messages,
    /// `POST /v1/generateContent`.
    GenerateContent,
}

impl LocalApiEndpoint {
    /// Parse the method/path pair used by the local HTTP and Unix-socket API.
    ///
    /// # Errors
    ///
    /// Returns [`LocalApiError::UnsupportedEndpoint`] for method/path pairs that
    /// are not part of the stable local API ABI.
    pub fn parse(method: &str, path: &str) -> Result<Self, LocalApiError> {
        match (method, path) {
            ("GET", "/v1/models") => Ok(Self::Models),
            ("POST", "/v1/chat/completions") => Ok(Self::ChatCompletions),
            ("POST", "/v1/responses") => Ok(Self::Responses),
            ("POST", "/v1/messages") => Ok(Self::Messages),
            ("POST", "/v1/generateContent") => Ok(Self::GenerateContent),
            _ => Err(LocalApiError::UnsupportedEndpoint {
                method: method.to_owned(),
                path: path.to_owned(),
            }),
        }
    }

    /// Return the API format used by request/response endpoints.
    #[must_use]
    pub const fn format(self) -> Option<ApiFormat> {
        match self {
            Self::Models => None,
            Self::ChatCompletions => Some(ApiFormat::OpenAiChat),
            Self::Responses => Some(ApiFormat::OpenAiResponses),
            Self::Messages => Some(ApiFormat::AnthropicMessages),
            Self::GenerateContent => Some(ApiFormat::GoogleGenerateContent),
        }
    }

    /// Canonical endpoint path.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::Models => "/v1/models",
            Self::ChatCompletions => "/v1/chat/completions",
            Self::Responses => "/v1/responses",
            Self::Messages => "/v1/messages",
            Self::GenerateContent => "/v1/generateContent",
        }
    }
}

/// Local API request after endpoint normalization.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LocalApiRequest {
    id: RequestId,
    endpoint: LocalApiEndpoint,
    provider: Option<ProviderId>,
    model: Option<ModelId>,
    thread: Option<ThreadId>,
    body: String,
}

impl LocalApiRequest {
    /// Build a normalized local API request.
    #[must_use]
    pub fn new(id: RequestId, endpoint: LocalApiEndpoint, body: impl Into<String>) -> Self {
        Self {
            id,
            endpoint,
            provider: None,
            model: None,
            thread: None,
            body: body.into(),
        }
    }

    /// Select a routed provider.
    #[must_use]
    pub fn with_provider(mut self, provider: ProviderId) -> Self {
        self.provider = Some(provider);
        self
    }

    /// Select a provider-local model.
    #[must_use]
    pub fn with_model(mut self, model: ModelId) -> Self {
        self.model = Some(model);
        self
    }

    /// Bind the request to a thread.
    #[must_use]
    pub fn with_thread(mut self, thread: ThreadId) -> Self {
        self.thread = Some(thread);
        self
    }

    /// Return the normalized local API endpoint.
    #[must_use]
    pub const fn endpoint(&self) -> LocalApiEndpoint {
        self.endpoint
    }

    /// Convert this normalized request into the daemon execution command.
    ///
    /// # Errors
    ///
    /// Returns [`LocalApiError::ModelsEndpointIsReadOnly`] for `GET /v1/models`,
    /// which is a discovery endpoint rather than a provider execution request.
    pub fn into_submit_request(self) -> Result<SubmitRequest, LocalApiError> {
        let Some(format) = self.endpoint.format() else {
            return Err(LocalApiError::ModelsEndpointIsReadOnly);
        };
        let mut command = SubmitRequest::new(self.id, format, self.body);
        if let Some(provider) = self.provider {
            command = command.with_provider(provider);
        }
        if let Some(model) = self.model {
            command = command.with_model(model);
        }
        if let Some(thread) = self.thread {
            command = command.with_thread(thread);
        }
        Ok(command)
    }
}

/// Local API front-door failure.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum LocalApiError {
    /// Method/path is outside the local API ABI.
    UnsupportedEndpoint {
        /// HTTP-style method.
        method: String,
        /// Request path.
        path: String,
    },
    /// `GET /v1/models` cannot be converted into a provider execution request.
    ModelsEndpointIsReadOnly,
}

impl Display for LocalApiError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::UnsupportedEndpoint {
                ref method,
                ref path,
            } => {
                write!(f, "unsupported local API endpoint: {method} {path}")
            }
            Self::ModelsEndpointIsReadOnly => f.write_str("models endpoint is read-only"),
        }
    }
}

impl Error for LocalApiError {}

/// Daemon execution plane for accepted native API requests.
#[derive(Debug)]
pub struct ExecutionPlane<S, P> {
    store: S,
    provider: P,
    queue: VecDeque<SubmitRequest>,
}

impl<S, P> ExecutionPlane<S, P> {
    /// Create an execution plane from a persistence store and provider adapter.
    #[must_use]
    pub const fn new(store: S, provider: P) -> Self {
        Self {
            store,
            provider,
            queue: VecDeque::new(),
        }
    }

    /// Borrow the persistence store.
    #[must_use]
    pub const fn store(&self) -> &S {
        &self.store
    }

    /// Current queued request count.
    #[must_use]
    pub fn queued_len(&self) -> usize {
        self.queue.len()
    }

    /// Request id of the next queued request.
    #[must_use]
    pub fn next_queued_id(&self) -> Option<&RequestId> {
        self.queue.front().map(|request| &request.id)
    }
}

impl<S, P> ExecutionPlane<S, P>
where
    S: Store,
    P: Provider,
{
    /// Handle one normalized local API request through the daemon front door.
    ///
    /// Discovery endpoints are answered from daemon/provider state. Native API
    /// request endpoints are converted into [`SubmitRequest`] and executed
    /// through the same queue path used by FUSE submissions.
    pub fn handle_local_api(
        &mut self,
        request: LocalApiRequest,
    ) -> Result<ApiResponse, ExecutionError> {
        let audit_format = request
            .endpoint
            .format()
            .map_or("local_api.models", ApiFormat::as_str);
        let mut event = AuditEvent::new(request.id.clone(), audit_format, "local_api")
            .with_decision("accepted");
        if let Some(provider) = request.provider.clone() {
            event = event.with_provider(provider);
        }
        if let Some(model) = request.model.clone() {
            event = event.with_model(model);
        }
        self.store.append_audit(event)?;
        if request.endpoint == LocalApiEndpoint::Models {
            return Ok(ApiResponse::new(request.id, self.model_list_response()));
        }
        self.submit(request.into_submit_request()?)
    }

    /// Accept one native API request into the daemon queue.
    pub fn enqueue(
        &mut self,
        mut command: SubmitRequest,
    ) -> Result<EnqueueOutcome, ExecutionError> {
        let request_id = command.id.clone();
        if self.store.response(&request_id).is_some() {
            return Ok(EnqueueOutcome::AlreadyCompleted(request_id));
        }
        if self.store.request(&request_id).is_some() {
            return Ok(EnqueueOutcome::AlreadyQueued(request_id));
        }
        command.ensure_provider(self.provider.id().clone());
        self.persist_request(&command)?;
        self.store
            .append_audit(command.audit_event(request_id.clone(), "queued", "ready"))?;
        self.queue.push_back(command);
        Ok(EnqueueOutcome::Queued(request_id))
    }

    /// Execute the next queued request, if any.
    pub fn drain_next(&mut self) -> Result<Option<ApiResponse>, ExecutionError> {
        let Some(command) = self.queue.pop_front() else {
            return Ok(None);
        };
        self.execute(&command).map(Some)
    }

    /// Synchronous helper for tests and early single-process integrations.
    pub fn submit(&mut self, command: SubmitRequest) -> Result<ApiResponse, ExecutionError> {
        let request_id = command.id.clone();
        match self.enqueue(command)? {
            EnqueueOutcome::Queued(_) => self.drain_next()?.ok_or(ExecutionError::EmptyQueue),
            EnqueueOutcome::AlreadyCompleted(_) => self
                .store
                .response(&request_id)
                .cloned()
                .ok_or(ExecutionError::EmptyQueue),
            EnqueueOutcome::AlreadyQueued(_) => Err(ExecutionError::DuplicatePendingRequest),
        }
    }

    fn persist_request(&mut self, command: &SubmitRequest) -> Result<(), ExecutionError> {
        let provider_id = command
            .provider
            .clone()
            .unwrap_or_else(|| self.provider.id().clone());
        let mut request = ApiRequest::new(
            command.id.clone(),
            command.format,
            Some(provider_id),
            command.body.clone(),
        );
        if let Some(model) = command.model.clone() {
            request = request.with_model(model);
        }
        self.store.put_request(request)?;
        Ok(())
    }

    fn execute(&mut self, command: &SubmitRequest) -> Result<ApiResponse, ExecutionError> {
        let mut provider_request = ProviderRequest::new(command.format, command.body.clone());
        if let Some(provider) = command.provider.clone() {
            provider_request = provider_request.with_provider(provider);
        }
        if let Some(model) = command.model.clone() {
            provider_request = provider_request.with_model(model);
        }
        let provider_response = match self.provider.call(provider_request) {
            Ok(response) => response,
            Err(error) => {
                self.store.append_audit(command.audit_event(
                    command.id.clone(),
                    "error",
                    "provider_error",
                ))?;
                return Err(error.into());
            }
        };
        let request_body = command.body.clone();
        let thread = command.thread.clone();
        let response = ApiResponse::new(command.id.clone(), provider_response.body().to_owned());
        self.store.put_response(response.clone())?;
        if let Some(thread) = thread {
            self.append_thread_messages(&thread, command.format, &request_body, response.body())?;
        }
        self.store.append_audit(command.audit_event(
            response.request_id().clone(),
            "drained",
            "ready",
        ))?;
        Ok(response)
    }

    fn model_list_response(&self) -> String {
        let provider = self.provider.id().as_str();
        let data = self
            .provider
            .models()
            .into_iter()
            .map(|model| {
                serde_json::json!({
                    "id": model.id().as_str(),
                    "object": "model",
                    "owned_by": provider,
                    "provider": provider,
                    "format": model.format().as_str(),
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "object": "list",
            "data": data,
        })
        .to_string()
    }

    fn append_thread_messages(
        &mut self,
        thread: &ThreadId,
        format: ApiFormat,
        request_body: &str,
        response_body: &str,
    ) -> Result<(), ExecutionError> {
        let user = request_user_text(format, request_body);
        let assistant = response_assistant_text(format, response_body);
        self.store
            .append_thread_message(thread.clone(), Message::new(MessageRole::User, user))?;
        self.store.append_thread_message(
            thread.clone(),
            Message::new(MessageRole::Assistant, assistant),
        )?;
        self.store.set_thread_fingerprint(
            thread.clone(),
            thread_fingerprint(request_body, response_body),
        )?;
        Ok(())
    }
}

fn request_user_text(format: ApiFormat, body: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return body.trim().to_owned();
    };
    match format {
        ApiFormat::OpenAiChat | ApiFormat::AnthropicMessages => value
            .get("messages")
            .and_then(serde_json::Value::as_array)
            .and_then(|messages| messages.iter().rev().find_map(message_content))
            .unwrap_or_default()
            .to_owned(),
        ApiFormat::OpenAiResponses => value
            .get("input")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        ApiFormat::GoogleGenerateContent => value
            .get("contents")
            .and_then(serde_json::Value::as_array)
            .and_then(|contents| contents.iter().rev().find_map(parts_text))
            .unwrap_or_default()
            .to_owned(),
    }
}

fn response_assistant_text(format: ApiFormat, body: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return body.trim().to_owned();
    };
    match format {
        ApiFormat::OpenAiChat => value
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(message_content)
            .unwrap_or_default()
            .to_owned(),
        ApiFormat::OpenAiResponses => value
            .get("output_text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| {
                value
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
            })
            .to_owned(),
        ApiFormat::AnthropicMessages => value
            .get("content")
            .and_then(serde_json::Value::as_array)
            .and_then(|content| content.iter().find_map(text_field))
            .unwrap_or_default()
            .to_owned(),
        ApiFormat::GoogleGenerateContent => value
            .get("candidates")
            .and_then(serde_json::Value::as_array)
            .and_then(|candidates| candidates.first())
            .and_then(|candidate| candidate.get("content"))
            .and_then(parts_text)
            .unwrap_or_default()
            .to_owned(),
    }
}

fn message_content(value: &serde_json::Value) -> Option<&str> {
    value.get("content").and_then(serde_json::Value::as_str)
}

fn parts_text(value: &serde_json::Value) -> Option<&str> {
    value
        .get("parts")
        .and_then(serde_json::Value::as_array)?
        .iter()
        .find_map(text_field)
}

fn text_field(value: &serde_json::Value) -> Option<&str> {
    value.get("text").and_then(serde_json::Value::as_str)
}

fn thread_fingerprint(request_body: &str, response_body: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in request_body.bytes().chain(response_body.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv1a64:{hash:016x}")
}

/// Queue acceptance result.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum EnqueueOutcome {
    /// Request was newly queued.
    Queued(RequestId),
    /// Request id is already known but has no stored response yet.
    AlreadyQueued(RequestId),
    /// Request id already has a stored response.
    AlreadyCompleted(RequestId),
}

/// Accepted request command passed into the daemon execution plane.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SubmitRequest {
    id: RequestId,
    format: ApiFormat,
    provider: Option<ProviderId>,
    model: Option<ModelId>,
    thread: Option<ThreadId>,
    body: String,
}

impl SubmitRequest {
    /// Build a request command.
    #[must_use]
    pub fn new(id: RequestId, format: ApiFormat, body: impl Into<String>) -> Self {
        Self {
            id,
            format,
            provider: None,
            model: None,
            thread: None,
            body: body.into(),
        }
    }

    /// Select a provider explicitly.
    #[must_use]
    pub fn with_provider(mut self, provider: ProviderId) -> Self {
        self.provider = Some(provider);
        self
    }

    /// Select a provider-local model explicitly.
    #[must_use]
    pub fn with_model(mut self, model: ModelId) -> Self {
        self.model = Some(model);
        self
    }

    /// Bind the request to a thread.
    #[must_use]
    pub fn with_thread(mut self, thread: ThreadId) -> Self {
        self.thread = Some(thread);
        self
    }

    fn ensure_provider(&mut self, provider: ProviderId) {
        if self.provider.is_none() {
            self.provider = Some(provider);
        }
    }

    fn audit_event(&self, request_id: RequestId, event: &str, decision: &str) -> AuditEvent {
        let mut audit = AuditEvent::new(request_id, self.format.as_str(), event)
            .with_decision(decision.to_owned());
        if let Some(provider) = self.provider.clone() {
            audit = audit.with_provider(provider);
        }
        if let Some(model) = self.model.clone() {
            audit = audit.with_model(model);
        }
        audit
    }
}

/// Execution failure.
#[derive(Debug)]
pub enum ExecutionError {
    /// Persistence failed.
    Store(StoreError),
    /// Provider execution failed.
    Provider(ProviderError),
    /// Queue was unexpectedly empty after accepting a request.
    EmptyQueue,
    /// Request id is already pending and should not be executed twice.
    DuplicatePendingRequest,
    /// Local API normalization failed.
    LocalApi(LocalApiError),
}

impl Display for ExecutionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Store(ref error) => error.fmt(f),
            Self::Provider(ref error) => error.fmt(f),
            Self::EmptyQueue => f.write_str("execution queue is empty"),
            Self::DuplicatePendingRequest => f.write_str("duplicate pending request"),
            Self::LocalApi(ref error) => error.fmt(f),
        }
    }
}

impl Error for ExecutionError {}

impl From<StoreError> for ExecutionError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<ProviderError> for ExecutionError {
    fn from(error: ProviderError) -> Self {
        Self::Provider(error)
    }
}

impl From<LocalApiError> for ExecutionError {
    fn from(error: LocalApiError) -> Self {
        Self::LocalApi(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EnqueueOutcome, ExecutionError, ExecutionPlane, LocalApiEndpoint, LocalApiError,
        LocalApiRequest, SubmitRequest,
    };
    use cortex_core::{ApiFormat, MessageRole, ProviderId, ThreadId};
    use cortex_providers::{InMemoryProvider, ProviderResponse};
    use cortex_store::{InMemoryStore, RequestId, Store};
    use std::error::Error;

    #[test]
    fn submit_persists_request_and_provider_response() -> Result<(), Box<dyn Error>> {
        let provider = InMemoryProvider::new(
            ProviderId::new("test-provider")?,
            vec![ApiFormat::OpenAiChat],
        )
        .with_response(
            ApiFormat::OpenAiChat,
            ProviderResponse::new(ApiFormat::OpenAiChat, r#"{"ok":true}"#),
        );
        let mut plane = ExecutionPlane::new(InMemoryStore::new(), provider);
        let request_id = RequestId::new("001");

        let response = plane.submit(SubmitRequest::new(
            request_id.clone(),
            ApiFormat::OpenAiChat,
            r#"{"messages":[]}"#,
        ))?;

        assert_eq!(response.body(), r#"{"ok":true}"#);
        assert!(
            plane.store().request(&request_id).is_some(),
            "request must be persisted before provider response"
        );
        assert_eq!(plane.store().response(&request_id), Some(&response));
        Ok(())
    }

    #[test]
    fn enqueue_persists_without_calling_provider_until_drain() -> Result<(), Box<dyn Error>> {
        let provider = InMemoryProvider::new(
            ProviderId::new("test-provider")?,
            vec![ApiFormat::OpenAiChat],
        )
        .with_response(
            ApiFormat::OpenAiChat,
            ProviderResponse::new(ApiFormat::OpenAiChat, r#"{"ok":true}"#),
        );
        let mut plane = ExecutionPlane::new(InMemoryStore::new(), provider);
        let request_id = RequestId::new("queued-1");

        let accepted = plane.enqueue(SubmitRequest::new(
            request_id.clone(),
            ApiFormat::OpenAiChat,
            r#"{"messages":[]}"#,
        ))?;

        assert_eq!(accepted, EnqueueOutcome::Queued(request_id.clone()));
        assert_eq!(plane.queued_len(), 1);
        assert!(plane.store().request(&request_id).is_some());
        assert!(plane.store().response(&request_id).is_none());

        let Some(response) = plane.drain_next()? else {
            return Err("queued request should produce a response".into());
        };

        assert_eq!(response.body(), r#"{"ok":true}"#);
        assert_eq!(plane.queued_len(), 0);
        assert_eq!(plane.store().response(&request_id), Some(&response));
        Ok(())
    }

    #[test]
    fn enqueue_persists_explicit_routed_provider() -> Result<(), Box<dyn Error>> {
        let provider = InMemoryProvider::new(
            ProviderId::new("execution-adapter")?,
            vec![ApiFormat::OpenAiResponses],
        )
        .with_response(
            ApiFormat::OpenAiResponses,
            ProviderResponse::new(ApiFormat::OpenAiResponses, r#"{"ok":true}"#),
        );
        let mut plane = ExecutionPlane::new(InMemoryStore::new(), provider);
        let request_id = RequestId::new("routed-provider");

        let accepted = plane.enqueue(
            SubmitRequest::new(
                request_id.clone(),
                ApiFormat::OpenAiResponses,
                r#"{"input":"hello"}"#,
            )
            .with_provider(ProviderId::new("relay-openai")?),
        )?;

        assert_eq!(accepted, EnqueueOutcome::Queued(request_id.clone()));
        let request = plane
            .store()
            .request(&request_id)
            .ok_or("request should be persisted")?;
        assert_eq!(
            request.provider().map(ProviderId::as_str),
            Some("relay-openai")
        );
        Ok(())
    }

    #[test]
    fn duplicate_request_id_is_idempotent_after_response() -> Result<(), Box<dyn Error>> {
        let provider = InMemoryProvider::new(
            ProviderId::new("test-provider")?,
            vec![ApiFormat::OpenAiChat],
        )
        .with_response(
            ApiFormat::OpenAiChat,
            ProviderResponse::new(ApiFormat::OpenAiChat, r#"{"ok":true}"#),
        );
        let mut plane = ExecutionPlane::new(InMemoryStore::new(), provider);
        let request_id = RequestId::new("same-id");
        let command = SubmitRequest::new(request_id, ApiFormat::OpenAiChat, r#"{"messages":[]}"#);

        let first = plane.submit(command.clone())?;
        let second = plane.submit(command)?;

        assert_eq!(first, second);
        assert_eq!(plane.queued_len(), 0);
        Ok(())
    }

    #[test]
    fn duplicate_pending_request_is_not_enqueued_twice() -> Result<(), Box<dyn Error>> {
        let provider = InMemoryProvider::new(
            ProviderId::new("test-provider")?,
            vec![ApiFormat::OpenAiChat],
        )
        .with_response(
            ApiFormat::OpenAiChat,
            ProviderResponse::new(ApiFormat::OpenAiChat, r#"{"ok":true}"#),
        );
        let mut plane = ExecutionPlane::new(InMemoryStore::new(), provider);
        let request_id = RequestId::new("pending-id");
        let command = SubmitRequest::new(
            request_id.clone(),
            ApiFormat::OpenAiChat,
            r#"{"messages":[]}"#,
        );

        assert_eq!(
            plane.enqueue(command.clone())?,
            EnqueueOutcome::Queued(request_id.clone())
        );
        assert_eq!(
            plane.enqueue(command)?,
            EnqueueOutcome::AlreadyQueued(request_id)
        );
        assert_eq!(plane.queued_len(), 1);
        assert_eq!(
            plane
                .submit(SubmitRequest::new(
                    RequestId::new("pending-id"),
                    ApiFormat::OpenAiChat,
                    r#"{"messages":[]}"#
                ))
                .map_err(|error| error.to_string()),
            Err(ExecutionError::DuplicatePendingRequest.to_string())
        );
        Ok(())
    }

    #[test]
    fn local_api_endpoint_maps_standard_paths_to_formats() -> Result<(), Box<dyn Error>> {
        assert_eq!(
            LocalApiEndpoint::parse("POST", "/v1/chat/completions")?.format(),
            Some(ApiFormat::OpenAiChat)
        );
        assert_eq!(
            LocalApiEndpoint::parse("POST", "/v1/responses")?.format(),
            Some(ApiFormat::OpenAiResponses)
        );
        assert_eq!(
            LocalApiEndpoint::parse("POST", "/v1/messages")?.format(),
            Some(ApiFormat::AnthropicMessages)
        );
        assert_eq!(
            LocalApiEndpoint::parse("POST", "/v1/generateContent")?.format(),
            Some(ApiFormat::GoogleGenerateContent)
        );
        assert_eq!(LocalApiEndpoint::parse("GET", "/v1/models")?.format(), None);
        Ok(())
    }

    #[test]
    fn local_api_rejects_noncanonical_endpoints() {
        assert_eq!(
            LocalApiEndpoint::parse("GET", "/v1/model"),
            Err(LocalApiError::UnsupportedEndpoint {
                method: "GET".to_owned(),
                path: "/v1/model".to_owned(),
            })
        );
    }

    #[test]
    fn local_api_request_builds_submit_request_with_route_overrides() -> Result<(), Box<dyn Error>>
    {
        let command = LocalApiRequest::new(
            RequestId::new("local-api-001"),
            LocalApiEndpoint::ChatCompletions,
            r#"{"messages":[]}"#,
        )
        .with_provider(ProviderId::new("relay-openai")?)
        .with_model(cortex_core::ModelId::new("gpt-test")?)
        .into_submit_request()?;

        let provider = InMemoryProvider::new(
            ProviderId::new("execution-adapter")?,
            vec![ApiFormat::OpenAiChat],
        )
        .with_response(
            ApiFormat::OpenAiChat,
            ProviderResponse::new(ApiFormat::OpenAiChat, r#"{"ok":true}"#),
        );
        let mut plane = ExecutionPlane::new(InMemoryStore::new(), provider);
        let accepted = plane.enqueue(command)?;

        assert_eq!(
            accepted,
            EnqueueOutcome::Queued(RequestId::new("local-api-001"))
        );
        let request = plane
            .store()
            .request(&RequestId::new("local-api-001"))
            .ok_or("request should be persisted")?;
        assert_eq!(request.format(), ApiFormat::OpenAiChat);
        assert_eq!(
            request.provider().map(ProviderId::as_str),
            Some("relay-openai")
        );
        assert_eq!(
            request.model().map(cortex_core::ModelId::as_str),
            Some("gpt-test")
        );
        Ok(())
    }

    #[test]
    fn local_api_models_endpoint_is_not_a_submit_request() {
        let result = LocalApiRequest::new(RequestId::new("models"), LocalApiEndpoint::Models, "")
            .into_submit_request();

        assert_eq!(result, Err(LocalApiError::ModelsEndpointIsReadOnly));
    }

    #[test]
    fn local_api_models_endpoint_returns_provider_model_list() -> Result<(), Box<dyn Error>> {
        let provider = InMemoryProvider::new(
            ProviderId::new("test-provider")?,
            vec![ApiFormat::OpenAiChat],
        )
        .with_models(vec![cortex_providers::ProviderModel::new(
            cortex_core::ModelId::new("test-model")?,
            ApiFormat::OpenAiChat,
        )]);
        let mut plane = ExecutionPlane::new(InMemoryStore::new(), provider);

        let response = plane.handle_local_api(LocalApiRequest::new(
            RequestId::new("models"),
            LocalApiEndpoint::Models,
            "",
        ))?;

        assert!(response.body().contains("\"object\":\"list\""));
        assert!(response.body().contains("\"id\":\"test-model\""));
        assert!(response.body().contains("\"provider\":\"test-provider\""));
        assert_eq!(plane.queued_len(), 0);
        let events = plane.store().audit_events();
        assert_eq!(events.len(), 1);
        let event = events.first().ok_or("missing audit event")?;
        assert_eq!(event.request_id(), &RequestId::new("models"));
        assert_eq!(event.format(), "local_api.models");
        assert_eq!(event.event(), "local_api");
        Ok(())
    }

    #[test]
    fn local_api_request_endpoint_enters_execution_queue_path() -> Result<(), Box<dyn Error>> {
        let provider = InMemoryProvider::new(
            ProviderId::new("test-provider")?,
            vec![ApiFormat::OpenAiChat],
        )
        .with_response(
            ApiFormat::OpenAiChat,
            ProviderResponse::new(ApiFormat::OpenAiChat, r#"{"ok":true}"#),
        );
        let mut plane = ExecutionPlane::new(InMemoryStore::new(), provider);

        let response = plane.handle_local_api(LocalApiRequest::new(
            RequestId::new("chat"),
            LocalApiEndpoint::ChatCompletions,
            r#"{"messages":[]}"#,
        ))?;

        assert_eq!(response.body(), r#"{"ok":true}"#);
        assert!(plane.store().request(&RequestId::new("chat")).is_some());
        let events = plane.store().audit_events();
        assert!(
            events
                .iter()
                .any(|event| event.event() == "local_api" && event.format() == "openai.chat")
        );
        assert!(events.iter().any(|event| event.event() == "queued"
            && event.format() == "openai.chat"
            && event.provider().map(ProviderId::as_str) == Some("test-provider")
            && event.decision() == Some("ready")));
        assert!(events.iter().any(|event| event.event() == "drained"
            && event.format() == "openai.chat"
            && event.provider().map(ProviderId::as_str) == Some("test-provider")
            && event.decision() == Some("ready")));
        Ok(())
    }

    #[test]
    fn local_api_front_door_has_no_unstored_provider_bypass() -> Result<(), Box<dyn Error>> {
        let provider = InMemoryProvider::new(
            ProviderId::new("test-provider")?,
            vec![ApiFormat::OpenAiResponses],
        )
        .with_response(
            ApiFormat::OpenAiResponses,
            ProviderResponse::new(ApiFormat::OpenAiResponses, r#"{"ok":true}"#),
        );
        let request_id = RequestId::new("local-api-store");
        let mut plane = ExecutionPlane::new(InMemoryStore::new(), provider);

        let response = plane.handle_local_api(LocalApiRequest::new(
            request_id.clone(),
            LocalApiEndpoint::Responses,
            r#"{"input":"hello"}"#,
        ))?;

        assert_eq!(response.body(), r#"{"ok":true}"#);
        assert!(plane.store().request(&request_id).is_some());
        assert_eq!(plane.store().response(&request_id), Some(&response));
        assert_eq!(plane.queued_len(), 0);
        assert!(
            plane
                .store()
                .audit_events()
                .iter()
                .any(|event| event.request_id() == &request_id && event.event() == "drained")
        );
        Ok(())
    }

    #[test]
    fn local_api_bound_thread_appends_messages_after_response() -> Result<(), Box<dyn Error>> {
        let provider = InMemoryProvider::new(
            ProviderId::new("test-provider")?,
            vec![ApiFormat::OpenAiChat],
        )
        .with_response(
            ApiFormat::OpenAiChat,
            ProviderResponse::new(
                ApiFormat::OpenAiChat,
                r#"{"choices":[{"message":{"content":"pong"}}]}"#,
            ),
        );
        let thread_id = ThreadId::new("demo")?;
        let mut plane = ExecutionPlane::new(InMemoryStore::new(), provider);

        let response = plane.handle_local_api(
            LocalApiRequest::new(
                RequestId::new("threaded-chat"),
                LocalApiEndpoint::ChatCompletions,
                r#"{"messages":[{"role":"user","content":"ping"}]}"#,
            )
            .with_thread(thread_id.clone()),
        )?;

        assert!(response.body().contains("pong"));
        let snapshot = plane.store().thread_snapshot(&thread_id)?;
        assert_eq!(snapshot.messages().len(), 2);
        let user = snapshot
            .messages()
            .first()
            .ok_or("missing user thread message")?;
        let assistant = snapshot
            .messages()
            .get(1)
            .ok_or("missing assistant thread message")?;
        assert_eq!(user.role(), MessageRole::User);
        assert_eq!(user.content(), "ping");
        assert_eq!(assistant.role(), MessageRole::Assistant);
        assert_eq!(assistant.content(), "pong");
        assert_eq!(snapshot.latest(), Some("pong"));
        assert!(
            snapshot
                .fingerprint()
                .is_some_and(|fingerprint| fingerprint.starts_with("fnv1a64:"))
        );
        Ok(())
    }

    #[test]
    fn local_api_provider_errors_are_audited() -> Result<(), Box<dyn Error>> {
        let provider = InMemoryProvider::new(
            ProviderId::new("test-provider")?,
            vec![ApiFormat::OpenAiResponses],
        );
        let request_id = RequestId::new("local-api-error");
        let mut plane = ExecutionPlane::new(InMemoryStore::new(), provider);

        let error = plane
            .handle_local_api(LocalApiRequest::new(
                request_id.clone(),
                LocalApiEndpoint::Responses,
                r#"{"input":"hello"}"#,
            ))
            .map_err(|error| error.to_string());

        assert!(error.is_err());
        assert!(
            plane
                .store()
                .audit_events()
                .iter()
                .any(|event| event.request_id() == &request_id
                    && event.event() == "error"
                    && event.provider().map(ProviderId::as_str) == Some("test-provider")
                    && event.decision() == Some("provider_error"))
        );
        Ok(())
    }
}
