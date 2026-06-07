#![forbid(unsafe_code)]

use cortex_core::{ApiFormat, Message, MessageRole, ModelId, ProviderId, ThreadId};
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Stable request identifier used for inbox/outbox idempotency.
#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub struct RequestId(String);

impl RequestId {
    /// Create a request identifier.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Return the identifier text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Native API request body accepted from an API format inbox.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ApiRequest {
    id: RequestId,
    format: ApiFormat,
    provider: Option<ProviderId>,
    model: Option<ModelId>,
    body: String,
}

impl ApiRequest {
    /// Create an API request record.
    #[must_use]
    pub fn new(
        id: RequestId,
        format: ApiFormat,
        provider: Option<ProviderId>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            id,
            format,
            provider,
            model: None,
            body: body.into(),
        }
    }

    /// Return a copy with the routed model set.
    #[must_use]
    pub fn with_model(mut self, model: ModelId) -> Self {
        self.model = Some(model);
        self
    }

    /// Request id.
    #[must_use]
    pub const fn id(&self) -> &RequestId {
        &self.id
    }

    /// API format used by the request.
    #[must_use]
    pub const fn format(&self) -> ApiFormat {
        self.format
    }

    /// Optional routed provider id.
    #[must_use]
    pub const fn provider(&self) -> Option<&ProviderId> {
        self.provider.as_ref()
    }

    /// Optional routed model id.
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

/// Native API response body written to an API format outbox.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ApiResponse {
    request_id: RequestId,
    body: String,
}

impl ApiResponse {
    /// Create an API response record.
    #[must_use]
    pub fn new(request_id: RequestId, body: impl Into<String>) -> Self {
        Self {
            request_id,
            body: body.into(),
        }
    }

    /// Request id that produced this response.
    #[must_use]
    pub const fn request_id(&self) -> &RequestId {
        &self.request_id
    }

    /// Raw native JSON response body.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }
}

/// Minimal audit event persisted by the execution store.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuditEvent {
    request_id: RequestId,
    format: String,
    event: String,
    provider: Option<ProviderId>,
    model: Option<ModelId>,
    decision: Option<String>,
}

impl AuditEvent {
    /// Create an audit event.
    #[must_use]
    pub fn new(request_id: RequestId, format: impl Into<String>, event: impl Into<String>) -> Self {
        Self {
            request_id,
            format: format.into(),
            event: event.into(),
            provider: None,
            model: None,
            decision: None,
        }
    }

    /// Return a copy with routed provider metadata.
    #[must_use]
    pub fn with_provider(mut self, provider: ProviderId) -> Self {
        self.provider = Some(provider);
        self
    }

    /// Return a copy with routed model metadata.
    #[must_use]
    pub fn with_model(mut self, model: ModelId) -> Self {
        self.model = Some(model);
        self
    }

    /// Return a copy with policy/routing decision metadata.
    #[must_use]
    pub fn with_decision(mut self, decision: impl Into<String>) -> Self {
        self.decision = Some(decision.into());
        self
    }

    /// Request id associated with the event.
    #[must_use]
    pub const fn request_id(&self) -> &RequestId {
        &self.request_id
    }

    /// API format or discovery surface.
    #[must_use]
    pub fn format(&self) -> &str {
        &self.format
    }

    /// Stable event name.
    #[must_use]
    pub fn event(&self) -> &str {
        &self.event
    }

    /// Routed provider, if known.
    #[must_use]
    pub const fn provider(&self) -> Option<&ProviderId> {
        self.provider.as_ref()
    }

    /// Routed model, if known.
    #[must_use]
    pub const fn model(&self) -> Option<&ModelId> {
        self.model.as_ref()
    }

    /// Policy/routing decision, if known.
    #[must_use]
    pub fn decision(&self) -> Option<&str> {
        self.decision.as_deref()
    }
}

/// Read-only snapshot of a thread state.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ThreadSnapshot {
    id: ThreadId,
    messages: Vec<Message>,
    latest: Option<String>,
    fingerprint: Option<String>,
}

impl ThreadSnapshot {
    /// Create a thread snapshot.
    #[must_use]
    pub fn new(
        id: ThreadId,
        messages: Vec<Message>,
        latest: Option<String>,
        fingerprint: Option<String>,
    ) -> Self {
        Self {
            id,
            messages,
            latest,
            fingerprint,
        }
    }

    /// Thread id.
    #[must_use]
    pub const fn id(&self) -> &ThreadId {
        &self.id
    }

    /// Stored messages.
    #[must_use]
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Latest assistant response, if present.
    #[must_use]
    pub fn latest(&self) -> Option<&str> {
        self.latest.as_deref()
    }

    /// Current canonical fingerprint, if computed.
    #[must_use]
    pub fn fingerprint(&self) -> Option<&str> {
        self.fingerprint.as_deref()
    }
}

/// Store failure.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum StoreError {
    /// Request id already exists.
    DuplicateRequest(RequestId),
    /// Request id is unknown.
    MissingRequest(RequestId),
    /// Thread id is unknown.
    MissingThread(ThreadId),
}

impl Display for StoreError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::DuplicateRequest(ref id) => write!(f, "duplicate request: {}", id.as_str()),
            Self::MissingRequest(ref id) => write!(f, "missing request: {}", id.as_str()),
            Self::MissingThread(ref id) => write!(f, "missing thread: {}", id.as_str()),
        }
    }
}

impl Error for StoreError {}

/// Store result.
pub type StoreResult<T> = Result<T, StoreError>;

/// Persistence API used by `cortexd` and projected by `cortexfs`.
pub trait Store {
    /// Persist an accepted API request.
    fn put_request(&mut self, request: ApiRequest) -> StoreResult<()>;

    /// Persist the response for an existing request.
    fn put_response(&mut self, response: ApiResponse) -> StoreResult<()>;

    /// Append an audit event.
    fn append_audit(&mut self, event: AuditEvent) -> StoreResult<()>;

    /// Append a chat message to a thread.
    fn append_thread_message(&mut self, id: ThreadId, message: Message) -> StoreResult<()>;

    /// Set the current thread fingerprint.
    fn set_thread_fingerprint(
        &mut self,
        id: ThreadId,
        fingerprint: impl Into<String>,
    ) -> StoreResult<()>;

    /// Read a request by id.
    fn request(&self, id: &RequestId) -> Option<&ApiRequest>;

    /// Read a response by request id.
    fn response(&self, id: &RequestId) -> Option<&ApiResponse>;

    /// Return persisted audit events.
    fn audit_events(&self) -> &[AuditEvent];

    /// Return a read-only thread snapshot.
    fn thread_snapshot(&self, id: &ThreadId) -> StoreResult<ThreadSnapshot>;
}

/// In-memory store placeholder for early daemon and FUSE integration tests.
#[derive(Debug, Default)]
pub struct InMemoryStore {
    requests: HashMap<RequestId, ApiRequest>,
    responses: HashMap<RequestId, ApiResponse>,
    threads: HashMap<ThreadId, ThreadState>,
    audit_events: Vec<AuditEvent>,
}

impl InMemoryStore {
    /// Create an empty in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl Store for InMemoryStore {
    fn put_request(&mut self, request: ApiRequest) -> StoreResult<()> {
        let id = request.id().clone();
        if self.requests.contains_key(&id) {
            return Err(StoreError::DuplicateRequest(id));
        }
        self.requests.insert(id, request);
        Ok(())
    }

    fn put_response(&mut self, response: ApiResponse) -> StoreResult<()> {
        if !self.requests.contains_key(response.request_id()) {
            return Err(StoreError::MissingRequest(response.request_id().clone()));
        }
        self.responses
            .insert(response.request_id().clone(), response);
        Ok(())
    }

    fn append_audit(&mut self, event: AuditEvent) -> StoreResult<()> {
        self.audit_events.push(event);
        Ok(())
    }

    fn append_thread_message(&mut self, id: ThreadId, message: Message) -> StoreResult<()> {
        let thread = self.threads.entry(id).or_default();
        if message.role() == MessageRole::Assistant {
            thread.latest = Some(message.content().to_owned());
        }
        thread.messages.push(message);
        Ok(())
    }

    fn set_thread_fingerprint(
        &mut self,
        id: ThreadId,
        fingerprint: impl Into<String>,
    ) -> StoreResult<()> {
        let thread = self
            .threads
            .get_mut(&id)
            .ok_or_else(|| StoreError::MissingThread(id.clone()))?;
        thread.fingerprint = Some(fingerprint.into());
        Ok(())
    }

    fn request(&self, id: &RequestId) -> Option<&ApiRequest> {
        self.requests.get(id)
    }

    fn response(&self, id: &RequestId) -> Option<&ApiResponse> {
        self.responses.get(id)
    }

    fn audit_events(&self) -> &[AuditEvent] {
        &self.audit_events
    }

    fn thread_snapshot(&self, id: &ThreadId) -> StoreResult<ThreadSnapshot> {
        let thread = self
            .threads
            .get(id)
            .ok_or_else(|| StoreError::MissingThread(id.clone()))?;
        Ok(ThreadSnapshot::new(
            id.clone(),
            thread.messages.clone(),
            thread.latest.clone(),
            thread.fingerprint.clone(),
        ))
    }
}

#[derive(Debug, Default)]
struct ThreadState {
    messages: Vec<Message>,
    latest: Option<String>,
    fingerprint: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{ApiRequest, ApiResponse, AuditEvent, InMemoryStore, RequestId, Store, StoreError};
    use cortex_core::{ApiFormat, Message, MessageRole, ModelId, ProviderId, ThreadId};
    use std::error::Error;

    #[test]
    fn request_response_lifecycle_requires_known_request() -> Result<(), Box<dyn Error>> {
        let mut store = InMemoryStore::new();
        let request_id = RequestId::new("req-1");
        let provider = ProviderId::new("kimi")?;
        let request = ApiRequest::new(
            request_id.clone(),
            ApiFormat::OpenAiChat,
            Some(provider),
            r#"{"messages":[]}"#,
        )
        .with_model(ModelId::new("gpt-test")?);

        store.put_request(request.clone())?;

        assert_eq!(store.request(&request_id), Some(&request));
        assert_eq!(
            store
                .request(&request_id)
                .and_then(ApiRequest::model)
                .map(ModelId::as_str),
            Some("gpt-test")
        );
        assert_eq!(
            store.put_request(request),
            Err(StoreError::DuplicateRequest(request_id.clone()))
        );
        assert_eq!(
            store.put_response(ApiResponse::new(RequestId::new("missing"), "{}")),
            Err(StoreError::MissingRequest(RequestId::new("missing")))
        );

        let response = ApiResponse::new(request_id.clone(), r#"{"ok":true}"#);
        store.put_response(response.clone())?;

        assert_eq!(store.response(&request_id), Some(&response));
        Ok(())
    }

    #[test]
    fn thread_snapshot_uses_core_message_abi() -> Result<(), Box<dyn Error>> {
        let mut store = InMemoryStore::new();
        let thread_id = ThreadId::new("demo-thread")?;

        assert_eq!(
            store.thread_snapshot(&thread_id),
            Err(StoreError::MissingThread(thread_id.clone()))
        );

        store.append_thread_message(thread_id.clone(), Message::new(MessageRole::User, "hello"))?;
        store.append_thread_message(
            thread_id.clone(),
            Message::new(MessageRole::Assistant, "world"),
        )?;
        store.set_thread_fingerprint(thread_id.clone(), "blake3:abc123")?;

        let snapshot = store.thread_snapshot(&thread_id)?;

        assert_eq!(snapshot.id(), &thread_id);
        assert_eq!(
            snapshot.messages(),
            [
                Message::new(MessageRole::User, "hello"),
                Message::new(MessageRole::Assistant, "world")
            ]
        );
        assert_eq!(snapshot.latest(), Some("world"));
        assert_eq!(snapshot.fingerprint(), Some("blake3:abc123"));
        Ok(())
    }

    #[test]
    fn store_keeps_append_only_audit_events() -> Result<(), Box<dyn Error>> {
        let mut store = InMemoryStore::new();
        let request_id = RequestId::new("audited");

        let provider = ProviderId::new("test-provider")?;
        let model = ModelId::new("test-model")?;
        store.append_audit(
            AuditEvent::new(request_id.clone(), ApiFormat::OpenAiChat.as_str(), "queued")
                .with_provider(provider.clone())
                .with_model(model.clone())
                .with_decision("ready"),
        )?;
        store.append_audit(AuditEvent::new(
            request_id.clone(),
            ApiFormat::OpenAiChat.as_str(),
            "drained",
        ))?;

        assert_eq!(
            store.audit_events(),
            [
                AuditEvent::new(request_id.clone(), "openai.chat", "queued")
                    .with_provider(provider)
                    .with_model(model)
                    .with_decision("ready"),
                AuditEvent::new(request_id, "openai.chat", "drained"),
            ]
        );
        let event = store.audit_events().first().ok_or("missing audit event")?;
        assert_eq!(
            event.provider().map(ProviderId::as_str),
            Some("test-provider")
        );
        assert_eq!(event.model().map(ModelId::as_str), Some("test-model"));
        assert_eq!(event.decision(), Some("ready"));
        Ok(())
    }
}
