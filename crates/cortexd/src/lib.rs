#![forbid(unsafe_code)]

use cortex_core::{ApiFormat, ModelId, ProviderId};
use cortex_providers::{Provider, ProviderError, ProviderRequest};
use cortex_store::{ApiRequest, ApiResponse, RequestId, Store, StoreError};
use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{Display, Formatter};

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
    /// Accept one native API request into the daemon queue.
    pub fn enqueue(&mut self, command: SubmitRequest) -> Result<EnqueueOutcome, ExecutionError> {
        let request_id = command.id.clone();
        if self.store.response(&request_id).is_some() {
            return Ok(EnqueueOutcome::AlreadyCompleted(request_id));
        }
        if self.store.request(&request_id).is_some() {
            return Ok(EnqueueOutcome::AlreadyQueued(request_id));
        }
        self.persist_request(&command)?;
        self.queue.push_back(command);
        Ok(EnqueueOutcome::Queued(request_id))
    }

    /// Execute the next queued request, if any.
    pub fn drain_next(&mut self) -> Result<Option<ApiResponse>, ExecutionError> {
        let Some(command) = self.queue.pop_front() else {
            return Ok(None);
        };
        self.execute(command).map(Some)
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
        let request = ApiRequest::new(
            command.id.clone(),
            command.format,
            Some(provider_id),
            command.body.clone(),
        );
        self.store.put_request(request)?;
        Ok(())
    }

    fn execute(&mut self, command: SubmitRequest) -> Result<ApiResponse, ExecutionError> {
        let mut provider_request = ProviderRequest::new(command.format, command.body);
        if let Some(provider) = command.provider {
            provider_request = provider_request.with_provider(provider);
        }
        if let Some(model) = command.model {
            provider_request = provider_request.with_model(model);
        }
        let provider_response = self.provider.call(provider_request)?;
        let response = ApiResponse::new(command.id, provider_response.body().to_owned());
        self.store.put_response(response.clone())?;
        Ok(response)
    }
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
}

impl Display for ExecutionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Store(ref error) => error.fmt(f),
            Self::Provider(ref error) => error.fmt(f),
            Self::EmptyQueue => f.write_str("execution queue is empty"),
            Self::DuplicatePendingRequest => f.write_str("duplicate pending request"),
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

#[cfg(test)]
mod tests {
    use super::{EnqueueOutcome, ExecutionError, ExecutionPlane, SubmitRequest};
    use cortex_core::{ApiFormat, ProviderId};
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
}
