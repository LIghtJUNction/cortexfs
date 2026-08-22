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
        model: "main".to_owned(),
        model_path: PathBuf::from("/tmp/cortexfs-test-ctx/model/main"),
        system_prompt: String::new(),
        prompt_template: DEFAULT_AGENT_PROMPT_TEMPLATE.to_owned(),
        rules: String::new(),
        skills: String::new(),
        current_time_unix: "123".to_owned(),
        tool_context: String::new(),
        history_messages: "previous message".to_owned(),
        window_setting: AgentWindowSetting::Auto,
        context_budget: None,
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
fn openai_provider_uses_resolved_credential_when_present() {
    let transport = ResolvedTransport::Direct {
        base_url: "https://api.example.test/v1".to_owned(),
    };
    let credential = ProviderCredential::Bearer("secret".to_owned());

    assert_eq!(
        openai_api_key(
            "api.example.test",
            transport_allows_unauthenticated(&transport),
            Some(&credential)
        ),
        Ok(Some("secret"))
    );
}

#[test]
fn brokered_external_direct_provider_still_requires_credential_before_request() {
    let original = ResolvedTransport::Direct {
        base_url: "https://api.example.test/v1".to_owned(),
    };
    let allow_unauthenticated = transport_allows_unauthenticated(&original);
    let rewritten = provider_egress_transport(
        "fixture",
        original,
        Some(OsStr::new(
            cortexfs::runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH,
        )),
    );

    assert_eq!(
        rewritten.map(|transport| (
            transport,
            openai_api_key("fixture", allow_unauthenticated, None)
        )),
        Ok((
            ResolvedTransport::Unix {
                base_url: "http://localhost/v1".to_owned(),
                socket_path: "/run/cortexfs/provider-egress/fixture.sock".to_owned(),
            },
            Err("missing provider credential: fixture".to_owned())
        ))
    );
}

#[test]
fn brokered_local_direct_provider_remains_anonymous() {
    let original = ResolvedTransport::Direct {
        base_url: "http://127.0.0.1:8317/v1".to_owned(),
    };
    let allow_unauthenticated = transport_allows_unauthenticated(&original);
    let rewritten = provider_egress_transport(
        "local",
        original,
        Some(OsStr::new(
            cortexfs::runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH,
        )),
    );

    assert_eq!(
        rewritten.map(|transport| (
            transport,
            openai_api_key("local", allow_unauthenticated, None)
        )),
        Ok((
            ResolvedTransport::Unix {
                base_url: "http://localhost/v1".to_owned(),
                socket_path: "/run/cortexfs/provider-egress/local.sock".to_owned(),
            },
            Ok(None)
        ))
    );
}

#[test]
fn brokered_provider_keeps_only_trusted_path_and_frozen_auth_policy() {
    let original = ResolvedTransport::Direct {
        base_url: "https://api.example.test/custom?ignored=yes".to_owned(),
    };
    let allow_unauthenticated = transport_allows_unauthenticated(&original);
    let rewritten = provider_egress_transport(
        "fixture",
        original,
        Some(OsStr::new(
            cortexfs::runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH,
        )),
    );

    assert_eq!(
        rewritten.map(|transport| (
            transport,
            openai_api_key("fixture", allow_unauthenticated, None)
        )),
        Ok((
            ResolvedTransport::Unix {
                base_url: "http://localhost/custom".to_owned(),
                socket_path: "/run/cortexfs/provider-egress/fixture.sock".to_owned(),
            },
            Err("missing provider credential: fixture".to_owned())
        ))
    );
}

#[test]
fn native_unix_provider_remains_anonymous() {
    let original = ResolvedTransport::Unix {
        base_url: "http://localhost/v1".to_owned(),
        socket_path: "/run/native.sock".to_owned(),
    };
    let allow_unauthenticated = transport_allows_unauthenticated(&original);
    let routed = provider_egress_transport(
        "local",
        original.clone(),
        Some(OsStr::new(
            cortexfs::runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH,
        )),
    );

    assert_eq!(
        routed.map(|transport| (
            transport,
            openai_api_key("local", allow_unauthenticated, None)
        )),
        Ok((original, Ok(None)))
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
    use std::os::unix::process::CommandExt;

    let root = unique_temp_dir("provider-oversized-stdout")?;
    let trigger = root.join("continue");
    let marker = root.join("survived");
    let child = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "sh -c 'while [ ! -f \"$GO\" ]; do sleep 1; done; printf survived > \"$MARKER\"' & head -c {} /dev/zero; wait",
            MAX_PROVIDER_RESPONSE_BYTES.saturating_add(1)
        ))
        .env("GO", &trigger)
        .env("MARKER", &marker)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .process_group(0)
        .spawn()?;

    let result = wait_for_curl_json_output(child);
    fs::write(&trigger, [])?;
    thread::sleep(Duration::from_secs(2));
    let marker_written = marker.exists();
    fs::remove_dir_all(root)?;

    assert!(matches!(result, Err(ref error) if error.contains("provider response exceeds")));
    assert!(!marker_written, "oversized provider child survived cleanup");
    Ok(())
}

#[test]
fn context_budget_parser_accepts_only_a_coherent_canonical_pair() {
    let parsed = parse_agent_context_budget(Some("16384"), Some("65536"));
    assert!(
        matches!(parsed, Ok(Some(value)) if value.tokens() == 16_384 && value.total_chars() == 65_536)
    );
    assert_eq!(parse_agent_context_budget(None, None), Ok(None));
    for (tokens, chars) in [
        (Some("1"), None),
        (None, Some("4")),
        (Some("01"), Some("4")),
        (Some("x"), Some("4")),
        (Some("1"), Some("04")),
        (Some("1"), Some("5")),
    ] {
        assert!(parse_agent_context_budget(tokens, chars).is_err());
    }
}

#[test]
fn prompt_admission_accepts_exact_boundary_and_rejects_one_more_byte() {
    let mut config = test_agent_run_config();
    config.context_budget = budget_from_effective(
        ModelContextLimit::known(4_096).unwrap_or(ModelContextLimit::Unknown),
    );
    let base = serialized_agent_messages(&config, "")
        .unwrap_or_default()
        .len();
    let input_budget = config
        .context_budget
        .map_or(0, AgentWindowBudget::input_chars);
    assert!(base < input_budget);
    let exact = "x".repeat(input_budget - base);
    assert_eq!(admit_agent_prompt(&config, &exact), Ok(true));
    assert_eq!(admit_agent_prompt(&config, &format!("{exact}x")), Ok(false));
}

#[test]
fn prompt_admission_includes_output_reservation_and_rechecks_tool_growth() {
    let mut config = test_agent_run_config();
    let budget = budget_from_effective(
        ModelContextLimit::known(4_096).unwrap_or(ModelContextLimit::Unknown),
    );
    config.context_budget = budget;
    assert!(
        matches!(budget, Some(value) if value.output_tokens() == 1_024 && value.input_chars() == 12_288)
    );
    assert_eq!(admit_agent_prompt(&config, "hello"), Ok(true));
    let frame = format!(
        "{}\n",
        serde_json::json!({
            "schema": cortexfs_runtime_client::agent::AGENT_INVOCATION_SCHEMA,
            "run": "r1",
            "step": 1,
            "input": "hello",
            "history_messages": "previous message",
            "tool_context": format!(
                "{}{}",
                crate::agent::TOOL_CALL_CONTEXT_PREFIX,
                serde_json::json!({
                    "id": "call-1", "name": "tsh", "arguments": {"args": ["tools"]}
                })
            ),
            "observation": {
                "tool_call_id": "call-1",
                "name": "tsh",
                "status": "ok",
                "content": "x".repeat(16_384),
                "truncated": false
            }
        })
    );
    let envelope = cortexfs_runtime_client::agent::read_agent_invocation(frame.as_bytes());
    assert!(
        envelope.is_ok(),
        "valid test envelope rejected: {envelope:?}"
    );
    let Some(envelope) = envelope.ok() else {
        return;
    };
    config.apply_invocation(&envelope);
    assert!(agent_continuation_messages(&config.tool_context).is_some());
    assert_eq!(admit_agent_prompt(&config, "hello"), Ok(false));
}

#[test]
fn unknown_window_preserves_legacy_prompt_admission() {
    let config = test_agent_run_config();
    assert_eq!(
        admit_agent_prompt(&config, &"x".repeat(128 * 1024)),
        Ok(true)
    );
}

#[test]
fn host_admission_serializes_the_shared_provider_messages_byte_for_byte() {
    let mut config = test_agent_run_config();
    config.system_prompt = "quote: \"雪\"".to_owned();
    config.tool_context = "tool\\result\n二".to_owned();
    let context = AgentPromptContext {
        template: config.prompt_template.clone(),
        rules: config.rules.clone(),
        skills: config.skills.clone(),
        tool_injection: config.tool_context.clone(),
        history_messages: config.history_messages.clone(),
        current_time_unix: config.current_time_unix.clone(),
    };
    let expected = provider_messages_for_agent(
        "user \"input\" 雪",
        Some(&config.agent),
        &config.system_prompt,
        &context,
    );
    assert_eq!(
        serialized_agent_messages(&config, "user \"input\" 雪"),
        serde_json::to_vec(&expected).map_err(|error| ExecError::new(error.to_string()))
    );
}

#[test]
fn recoverable_candidate_error_does_not_make_later_success_terminal() {
    let frames = vec![
        serde_json::json!({"type":"error", "code":"E2BIG", "recoverable":true}).to_string(),
        serde_json::json!({"type":"message", "role":"assistant", "content":[]}).to_string(),
    ];
    assert!(!frames_have_error(&frames));
    let terminal = vec![serde_json::json!({"type":"error", "code":"EIO"}).to_string()];
    assert!(frames_have_error(&terminal));
}
use super::config::test_provider_config_with_formats;
use super::*;
