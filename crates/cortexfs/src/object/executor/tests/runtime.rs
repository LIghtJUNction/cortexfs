pub(super) fn test_prompt_context() -> AgentPromptContext {
    AgentPromptContext {
        template: DEFAULT_AGENT_PROMPT_TEMPLATE.to_owned(),
        rules: "Project rule".to_owned(),
        skills: "- name: rust\n  description: Rust help\n  path: /skills/rust/SKILL.md\n"
            .to_owned(),
        tool_injection: "tool output".to_owned(),
        history_messages: "previous message".to_owned(),
        current_time_unix: "123".to_owned(),
    }
}

pub(super) fn test_agent_run_config() -> AgentModelRunConfig {
    AgentModelRunConfig {
        agent: "coder".to_owned(),
        source: PathBuf::from("/tmp/cortexfs-test-source"),
        ctx_root: PathBuf::from("/tmp/cortexfs-test-ctx"),
        run: "r1".to_owned(),
        session: "default".to_owned(),
        model: "main".to_owned(),
        model_path: PathBuf::from("/tmp/cortexfs-test-ctx/model/main"),
        system_prompt: String::new(),
        prompt_template: DEFAULT_AGENT_PROMPT_TEMPLATE.to_owned(),
        rules: String::new(),
        skills: String::new(),
        current_time_unix: "123".to_owned(),
        tool_context: String::new(),
        history_messages: "previous message".to_owned(),
        suppress_model_error_events: false,
        debug_timing_start_unix_ms: None,
    }
}
#[test]
fn provider_runtime_driver_uses_responses_for_openai_agent_calls() {
    let config = test_provider_config_with_formats(
        "https://api.example.test/v1",
        &["openai.chat", "openai.responses"],
    );

    assert_eq!(
        provider_runtime_driver(&config, false),
        ProviderRuntimeDriver::OpenAiChat
    );
    assert_eq!(
        provider_runtime_driver(&config, true),
        ProviderRuntimeDriver::OpenAiResponses
    );
}

#[test]
fn provider_runtime_driver_uses_responses_when_chat_is_absent() {
    let config =
        test_provider_config_with_formats("https://api.example.test/v1", &["openai.responses"]);

    assert_eq!(
        provider_runtime_driver(&config, false),
        ProviderRuntimeDriver::OpenAiResponses
    );
}

#[test]
fn openai_public_http_provider_requires_credential_before_curl() {
    let transport = ResolvedTransport::Direct {
        base_url: "https://api.example.test/v1".to_owned(),
    };

    assert_eq!(
        openai_api_key("api.example.test", &transport, None),
        Err("missing provider credential: api.example.test".to_owned())
    );
}

#[test]
fn openai_local_http_provider_allows_missing_credential() {
    let transport = ResolvedTransport::Direct {
        base_url: "http://127.0.0.1:8317/v1".to_owned(),
    };

    assert_eq!(openai_api_key("local", &transport, None), Ok(None));
}

#[test]
fn openai_provider_uses_resolved_credential_when_present() {
    let transport = ResolvedTransport::Direct {
        base_url: "https://api.example.test/v1".to_owned(),
    };
    let credential = ProviderCredential::Bearer("secret".to_owned());

    assert_eq!(
        openai_api_key("api.example.test", &transport, Some(&credential)),
        Ok(Some("secret"))
    );
}

#[test]
fn provider_request_failure_includes_response_body() {
    let output = std::process::Output {
        status: std::os::unix::process::ExitStatusExt::from_raw(22 << 8),
        stdout: br#"{"error":"Missing API key"}"#.to_vec(),
        stderr: Vec::new(),
    };

    assert_eq!(
        provider_request_failure_message(&output),
        r#"provider request failed with exit status: 22: {"error":"Missing API key"}"#
    );
}

#[test]
fn provider_curl_output_rejects_oversized_stdout() -> Result<(), Box<dyn std::error::Error>> {
    let child = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "yes x | head -c {}",
            MAX_PROVIDER_RESPONSE_BYTES.saturating_add(1)
        ))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let result = wait_for_curl_json_output(child);

    assert!(matches!(result, Err(ref error) if error.contains("provider response exceeds")));
    Ok(())
}

#[test]
fn provider_curl_output_kills_child_after_oversized_stdout()
-> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    let child = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "yes x | head -c {}; sleep 5",
            MAX_PROVIDER_RESPONSE_BYTES.saturating_add(1)
        ))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let result = wait_for_curl_json_output(child);

    assert!(matches!(result, Err(ref error) if error.contains("provider response exceeds")));
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}
use super::config::test_provider_config_with_formats;
use super::*;
