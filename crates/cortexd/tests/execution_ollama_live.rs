#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use cortex_core::{ApiFormat, MessageRole, ThreadId};
    use cortex_providers::OllamaProvider;
    use cortex_store::{InMemoryStore, RequestId, Store};
    use cortexd::{EnqueueOutcome, ExecutionPlane, SubmitRequest};

    #[test]
    #[ignore = "requires local Ollama with smollm2:135m"]
    fn smollm2_request_drains_through_execution_plane() -> Result<(), Box<dyn std::error::Error>> {
        let url = std::env::var("CORTEXFS_LIVE_URL")?;
        let provider = OllamaProvider::fixture_smollm2(url)?;
        let mut plane = ExecutionPlane::new(InMemoryStore::new(), provider);
        let request_id = RequestId::new("ollama-live-001");
        let thread_id = ThreadId::new("demo")?;
        let command = SubmitRequest::new(
            request_id.clone(),
            ApiFormat::OpenAiChat,
            serde_json::json!({
                "model": "smollm2:135m",
                "messages": [
                    {
                        "role": "user",
                        "content": "Reply with exactly: cortexfs-ok"
                    }
                ]
            })
            .to_string(),
        )
        .with_thread(thread_id.clone());

        assert_eq!(
            plane.enqueue(command)?,
            EnqueueOutcome::Queued(request_id.clone())
        );
        assert_eq!(plane.queued_len(), 1);
        assert!(
            plane.store().request(&request_id).is_some(),
            "daemon store must persist the request before provider execution"
        );
        assert!(plane.store().response(&request_id).is_none());

        let response = plane
            .drain_next()?
            .ok_or("queued Ollama request should produce a response")?;
        assert_eq!(plane.queued_len(), 0);
        assert_eq!(plane.store().response(&request_id), Some(&response));

        let body = serde_json::from_str::<serde_json::Value>(response.body())?;
        let model = body
            .get("model")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let content = body
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();

        assert!(model.starts_with("smollm2"), "unexpected model: {model:?}");
        assert!(!content.trim().is_empty(), "Ollama returned empty content");
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
        assert_eq!(user.content(), "Reply with exactly: cortexfs-ok");
        assert_eq!(assistant.role(), MessageRole::Assistant);
        assert_eq!(assistant.content(), content);
        assert_eq!(snapshot.latest(), Some(content));
        assert!(
            snapshot
                .fingerprint()
                .is_some_and(|fingerprint| fingerprint.starts_with("fnv1a64:"))
        );
        Ok(())
    }
}
