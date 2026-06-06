#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use cortex_core::ApiFormat;
    use cortex_providers::{OllamaProvider, Provider, ProviderRequest};

    #[test]
    #[ignore = "requires local Ollama with smollm2:135m"]
    fn smollm2_chat_smoke_returns_openai_chat_completion() -> Result<(), Box<dyn std::error::Error>>
    {
        let provider = OllamaProvider::local_smollm2()?;
        let response = provider.call(ProviderRequest::new(
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
        ))?;

        let body = serde_json::from_str::<serde_json::Value>(response.body())?;
        let content = body
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let model = body
            .get("model")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();

        assert!(model.starts_with("smollm2"), "unexpected model: {model:?}");
        assert!(!content.trim().is_empty(), "Ollama returned empty content");
        assert_eq!(
            body.get("object").and_then(serde_json::Value::as_str),
            Some("chat.completion")
        );
        Ok(())
    }
}
