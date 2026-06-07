#![forbid(unsafe_code)]
#![allow(
    clippy::tests_outside_test_module,
    reason = "integration test crates are themselves test modules"
)]

#[cfg(all(test, feature = "live-tests"))]
mod tests {
    use cortexfs::live_support::{LiveCortexFs, LivePipelineOutput};

    #[test]
    #[ignore = "requires local Ollama with smollm2:135m"]
    fn smollm2_request_drains_through_cortexfs_file_pipeline()
    -> Result<(), Box<dyn std::error::Error>> {
        let fs = LiveCortexFs::new();
        fs.use_ollama_execution_plane()?;
        fs.submit_api_request(
            "openai.chat",
            "ollama-live",
            "{\"model\":\"smollm2:135m\",\"messages\":[{\"role\":\"user\",\"content\":\"Reply with exactly: cortexfs-ok\"}]}\n",
        )?;

        assert_eq!(fs.read_path(["control", "queue_depth"])?, "1\n");
        fs.drain_once()?;

        let response = match fs.read_api_output("openai.chat", "ollama-live")? {
            LivePipelineOutput::Response(response) => response,
            LivePipelineOutput::Error(error) => return Err(error.into()),
        };
        let body = serde_json::from_str::<serde_json::Value>(&response)?;
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
        assert_eq!(fs.read_path(["control", "queue_depth"])?, "0\n");
        assert_eq!(fs.read_path(["control", "last_drained"])?, "ollama-live\n");
        let export = fs.read_path(["home", "1000", "export", "conversations.jsonl"])?;
        assert!(export.contains("\"request_id\":\"ollama-live\""));
        assert!(export.contains("smollm2:135m"));
        Ok(())
    }
}
