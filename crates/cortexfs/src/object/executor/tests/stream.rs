#[test]
fn provider_text_tool_call_writes_canonical_event() {
    let mut output = Vec::new();
    let result = write_model_text_or_tool_call(
        &mut output,
        "run-1",
        r#"{"type":"tool_call","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}"#,
    );

    assert!(result.is_ok());
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""type":"tool_call""#));
    assert!(output.contains(r#""run":"run-1""#));
    assert!(output.contains(r#""name":"tsh""#));
    assert!(!output.contains(r#""type":"delta""#));
}

#[test]
fn cli_tool_mode_outputs_plain_text() {
    let path = std::env::temp_dir().join(format!("cortexfs-runner-cli-{}", std::process::id()));
    assert!(fs::write(&path, "plain").is_ok());
    let mut output = Vec::new();
    let result = run_core_tool_cli("fs.read", &[OsString::from(&path)], &mut output);
    assert!(matches!(result, Ok(Some(code)) if code == std::process::ExitCode::SUCCESS));
    assert_eq!(String::from_utf8(output).unwrap_or_default(), "plain");
    let _ignored = fs::remove_file(path);
}

#[test]
fn openai_stream_event_extracts_chat_tool_call_delta() {
    let event = openai_stream_event(r#"data: {"choices":[{"delta":{"content":"hel"}}]}"#)
        .map(|frame| frame.event);
    assert!(matches!(event, Ok(OpenAiStreamEvent::Delta(text)) if text == "hel"));

    let event = openai_stream_event(
        r#"data: {"choices":[{"delta":{"tool_calls":[{"id":"call-abc","type":"function","function":{"name":"tsh","arguments":"{\"args\""}}]}}]}"#,
    )
    .map(|frame| frame.event);
    assert!(matches!(
        event,
        Ok(OpenAiStreamEvent::ToolCallDelta(delta))
            if delta.id.as_deref() == Some("call-abc")
                && delta.name.as_deref() == Some("tsh")
                && delta.arguments == r#"{"args""#
    ));

    assert!(matches!(
        openai_stream_event(r#"data: {"choices":[{"finish_reason":"tool_calls"}]}"#)
            .map(|frame| frame.event),
        Ok(OpenAiStreamEvent::ToolCallsDone)
    ));
}

#[test]
fn openai_stream_tool_call_stream_accumulates_canonical_tool_call()
-> Result<(), Box<dyn std::error::Error>> {
    let mut stream = OpenAiToolCallStream::default();
    let first = openai_stream_event(
        r#"data: {"choices":[{"delta":{"tool_calls":[{"id":"call-abc","type":"function","function":{"name":"tsh","arguments":"{\"args\""}}]}}]}"#,
    )?
    .event;
    let second = openai_stream_event(
        r#"data: {"choices":[{"delta":{"tool_calls":[{"type":"function","function":{"arguments":":[\"tools\"]}"}}]}}]}"#,
    )?
    .event;

    if let OpenAiStreamEvent::ToolCallDelta(delta) = first {
        stream.push(delta);
    }
    if let OpenAiStreamEvent::ToolCallDelta(delta) = second {
        stream.push(delta);
    }

    assert_eq!(
        stream.finish()?,
        Some(
            r#"{"arguments":{"args":["tools"]},"id":"call-abc","name":"tsh","type":"tool_call"}"#
                .to_owned()
        )
    );
    Ok(())
}

#[test]
fn openai_stream_event_extracts_usage() {
    let event =
        openai_stream_event(r#"data: {"usage":{"prompt_tokens":12,"completion_tokens":5}}"#)
            .map(|frame| frame.event);
    assert!(matches!(
        event,
        Ok(OpenAiStreamEvent::Usage(TokenUsage {
            input_tokens: 12,
            output_tokens: 5
        }))
    ));
}

#[test]
fn openai_stream_event_accepts_done_marker() {
    assert!(matches!(
        openai_stream_event("data: [DONE]").map(|frame| frame.event),
        Ok(OpenAiStreamEvent::Done)
    ));
}

#[test]
fn provider_stream_reader_rejects_oversized_line() {
    let input = format!("{}\n", "x".repeat(MAX_PROVIDER_STREAM_LINE_BYTES));
    let mut reader = Cursor::new(input.into_bytes());

    let result = read_provider_stream_line(&mut reader);

    assert!(result.is_err());
}

#[test]
fn openai_stream_event_extracts_responses_delta_text() {
    let event = openai_stream_event(r#"data: {"type":"response.output_text.delta","delta":"hel"}"#)
        .map(|frame| frame.event);
    assert!(matches!(event, Ok(OpenAiStreamEvent::Delta(text)) if text == "hel"));
}

#[test]
fn openai_stream_event_extracts_responses_done_text() {
    let event = openai_stream_event(r#"data: {"type":"response.output_text.done","text":"hel"}"#)
        .map(|frame| frame.event);
    assert!(matches!(event, Ok(OpenAiStreamEvent::FinalText(text)) if text == "hel"));

    let event = openai_stream_event(
        r#"data: {"type":"response.content_part.done","part":{"type":"output_text","text":"lo"}}"#,
    )
    .map(|frame| frame.event);
    assert!(matches!(event, Ok(OpenAiStreamEvent::FinalText(text)) if text == "lo"));

    let event = openai_stream_event(
        r#"data: {"type":"response.output_item.done","item":{"content":[{"type":"output_text","text":"ok"}]}}"#,
    )
    .map(|frame| frame.event);
    assert!(matches!(event, Ok(OpenAiStreamEvent::FinalText(text)) if text == "ok"));
}

#[test]
fn responses_done_text_is_not_treated_as_stream_delta() -> Result<(), Box<dyn std::error::Error>> {
    let events = [
        openai_stream_event(r#"data: {"type":"response.output_text.delta","delta":"Hi!"}"#)
            .map(|frame| frame.event),
        openai_stream_event(r#"data: {"type":"response.output_text.done","text":"Hi!"}"#)
            .map(|frame| frame.event),
    ];
    let mut output = Vec::new();
    let mut emitter = OpenAiStreamTextEmitter::new("run-1");
    let mut has_delta = false;

    for event in events {
        match event? {
            OpenAiStreamEvent::Delta(text) if !text.is_empty() => {
                emitter.push(&mut output, &text)?;
                has_delta = true;
            }
            OpenAiStreamEvent::FinalText(text) if !has_delta && !text.is_empty() => {
                emitter.push(&mut output, &text)?;
                has_delta = true;
            }
            _ => {}
        }
    }
    emitter.finish(&mut output)?;

    let rendered = String::from_utf8(output)?;
    assert_eq!(rendered.matches("Hi!").count(), 1);
    Ok(())
}

#[test]
fn openai_stream_event_accepts_responses_completed_marker() {
    assert!(matches!(
        openai_stream_event(r#"data: {"type":"response.completed"}"#).map(|frame| frame.event),
        Ok(OpenAiStreamEvent::Done)
    ));
}

#[test]
fn openai_stream_event_accepts_responses_done_marker() {
    assert!(matches!(
        openai_stream_event(r#"data: {"type":"response.done"}"#).map(|frame| frame.event),
        Ok(OpenAiStreamEvent::Done)
    ));
}

#[test]
fn openai_stream_event_reports_responses_failed_marker() {
    assert_eq!(
        openai_stream_event(
            r#"data: {"type":"response.failed","error":{"message":"quota exceeded"}}"#
        )
        .map(|frame| matches!(frame.event, OpenAiStreamEvent::Ignore)),
        Err("quota exceeded".to_owned())
    );
}

#[test]
fn openai_stream_event_does_not_mix_reasoning_into_answer_text() {
    let event =
        openai_stream_event(r#"data: {"choices":[{"delta":{"reasoning_content":"hidden"}}]}"#)
            .map(|frame| frame.event);
    assert!(matches!(event, Ok(OpenAiStreamEvent::Delta(text)) if text.is_empty()));
}

#[test]
fn openai_response_content_prefers_output_text() {
    assert_eq!(
        parse_openai_response_content(br#"{"output_text":"hello codex"}"#),
        Ok("hello codex".to_owned())
    );
}

#[test]
fn provider_usage_accepts_openai_and_anthropic_shapes() {
    assert_eq!(
        token_usage_from_value(&serde_json::json!({
            "usage": {"prompt_tokens": 10, "completion_tokens": 3}
        })),
        Some(TokenUsage {
            input_tokens: 10,
            output_tokens: 3,
        })
    );
    assert_eq!(
        token_usage_from_value(&serde_json::json!({
            "usage": {"input_tokens": 7, "output_tokens": 2}
        })),
        Some(TokenUsage {
            input_tokens: 7,
            output_tokens: 2,
        })
    );
    assert_eq!(
        token_usage_from_value(&serde_json::json!({
            "response": {
                "usage": {"input_tokens": 9, "output_tokens": 4}
            }
        })),
        Some(TokenUsage {
            input_tokens: 9,
            output_tokens: 4,
        })
    );
}

#[test]
fn openai_request_bodies_include_non_auto_effort() {
    let chat = openai_chat_body("gpt-5.6", "hello", false, cortexfs::ModelEffort::High);
    let responses = openai_responses_body("gpt-5.6", "hello", true, cortexfs::ModelEffort::XHigh);

    assert!(chat.contains(r#""reasoning":{"effort":"high"}"#));
    assert!(responses.contains(r#""reasoning":{"effort":"xhigh"}"#));
    assert!(
        !openai_chat_body("gpt-5.6", "hello", false, cortexfs::ModelEffort::Auto)
            .contains("reasoning")
    );
}

#[test]
fn openai_agent_chat_body_declares_tsh_tool() {
    let body = openai_chat_body_with_agent_tools(
        "gpt-5.6",
        "what tools?",
        true,
        cortexfs::ModelEffort::Auto,
        true,
    );
    assert!(body.contains(r#""name":"tsh""#), "{body}");
    assert!(body.contains(r#""tool_choice":"auto""#), "{body}");
    assert!(
        body.contains(r#""stream_options":{"include_usage":true}"#),
        "{body}"
    );
    assert!(body.contains(r#""required":["args"]"#), "{body}");
}

#[test]
fn openai_chat_tool_call_response_parses_as_canonical_tool_call() {
    let content = parse_openai_chat_content(
        br#"{"choices":[{"message":{"tool_calls":[{"id":"call-abc","type":"function","function":{"name":"tsh","arguments":"{\"args\":[\"tools\"]}"}}]}}]}"#,
    );
    assert_eq!(
        content,
        Ok(
            r#"{"arguments":{"args":["tools"]},"id":"call-abc","name":"tsh","type":"tool_call"}"#
                .to_owned()
        )
    );
}

#[test]
fn openai_response_content_parses_output_parts() {
    assert_eq!(
        parse_openai_response_content(
            br#"{"output":[{"content":[{"type":"output_text","text":"hello "},{"type":"output_text","text":"codex"}]}]}"#
        ),
        Ok("hello codex".to_owned())
    );
}

#[test]
fn agent_provider_messages_expose_only_tsh_as_native_tool() {
    let messages = provider_messages_for_agent(
        "what tools?",
        Some("coder"),
        "Always answer tersely.",
        &test_prompt_context(),
    );
    let system = messages
        .pointer("/0/content")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(system.contains("`tsh` is always native"));
    assert!(system.contains("tsh cache state never makes a tool direct-native"));
    assert!(system.contains("## AGENT Instructions"));
    assert!(system.contains("Always answer tersely."));
    assert!(system.contains("Project rule"));
    assert!(system.contains("name: rust"));
    assert!(system.contains("tool output"));
    assert!(system.contains("previous message"));
    assert!(system.contains("Do not claim direct access"));
    assert!(!system.contains("output this exact tool call"));
    assert!(system.contains("When tool execution is useful"));
    assert!(system.contains("inspect current files before editing"));
    assert!(default_agent_tool_context().contains("`/workspace`"));
    assert!(
        system.find("## Runtime Contract").unwrap_or(usize::MAX)
            < system.find("## AGENT Instructions").unwrap_or(usize::MAX)
    );
    assert_eq!(
        messages
            .pointer("/1/content")
            .and_then(serde_json::Value::as_str),
        Some("what tools?")
    );

    let prompt = render_agent_system_prompt("coder", "", &test_prompt_context());
    assert!(prompt.contains("CortexFS agent `coder`"));
    assert!(prompt.contains("Do not mention hidden platform tools such as `image_gen`"));
}

#[test]
fn agent_prompt_context_renders_workspace_runtime_hint() {
    let mut prompt_context = test_prompt_context();
    prompt_context.tool_injection = default_agent_tool_context();
    let prompt = render_agent_system_prompt("coder", "", &prompt_context);

    assert!(prompt.contains("`/workspace`"));
    assert!(prompt.contains("`CTX_SOURCE`"));
    assert!(prompt.contains("`CTX_ROOT`"));
}

#[test]
fn agent_prompt_template_controls_rendered_system_message() {
    let mut context = test_prompt_context();
    context.template =
        "agent={{agent}}\ninstructions={{agent_instructions}}\ncontract={{runtime_contract}}\n"
            .to_owned();

    let prompt = render_agent_system_prompt("coder", "custom identity", &context);

    assert!(prompt.starts_with("agent=coder\ninstructions=custom identity\n"));
    assert!(prompt.contains("contract=You are CortexFS agent `coder`."));
    assert!(!prompt.contains("## Rules"));
}

#[cfg(unix)]
#[test]
fn provider_partial_eof_is_nonfallback_without_retry() -> Result<(), Box<dyn std::error::Error>> {
    const CHILD_ENV: &str = "CORTEXFS_TEST_PROVIDER_PARTIAL_EOF";
    if std::env::var_os(CHILD_ENV).is_some() {
        let mut output = Vec::new();
        let result =
            runner::provider_chat_completion("fixture/model", "hello", "run-1", &mut output);
        let Err(error) = result else {
            return Err(
                std::io::Error::other("partial provider stream unexpectedly succeeded").into(),
            );
        };
        assert!(!error.can_fallback, "{}", error.message);
        return Ok(());
    }

    let (base_url, stop, server) = spawn_provider_sse(concat!(
        r#"data: {"choices":[{"delta":{"content":"partial"}}]}"#,
        "\n\n"
    ))?;
    let root = unique_temp_dir("runner-provider-partial-eof")?;
    let providers = root.join("providers.d");
    fs::create_dir_all(&providers)?;
    fs::write(
        providers.join("fixture.json"),
        format!(
            "{{\"name\":\"fixture\",\"base_url\":\"{base_url}/v1\",\"formats\":[\"openai.chat\"]}}\n"
        ),
    )?;
    let status = std::process::Command::new(std::env::current_exe()?)
        .arg("provider_partial_eof_is_nonfallback_without_retry")
        .arg("--nocapture")
        .env(CHILD_ENV, "1")
        .env("CTX_PROVIDER_CONFIG_DIR", &providers)
        .env("CTX_ROOT", &root)
        .env_remove("CTX_AGENT")
        .status()?;
    let _ignored = stop.send(());
    let requests = server
        .join()
        .map_err(|_panic| std::io::Error::other("provider test server panicked"))??;
    let _ignored = fs::remove_dir_all(root);

    assert_eq!(requests, 1, "partial EOF must not trigger a second request");
    assert!(status.success(), "partial EOF child assertion failed");
    Ok(())
}

#[cfg(unix)]
#[test]
fn brokered_external_provider_stops_all_openai_drivers_before_request()
-> Result<(), Box<dyn std::error::Error>> {
    const CHILD_ENV: &str = "CORTEXFS_TEST_BROKERED_PROVIDER_AUTH";
    const PROVIDER_ENV: &str = "CORTEXFS_TEST_BROKERED_PROVIDER_NAME";
    if std::env::var_os(CHILD_ENV).is_some() {
        let provider = std::env::var(PROVIDER_ENV)?;
        assert!(matches!(
            cortexfs::read_provider_system_secret(&provider, "default"),
            Ok(None) | Err(cortexfs::ProviderSystemSecretError::CannotRead)
        ));
        reset_provider_request_attempts();
        let mut output = Vec::new();
        let result = runner::provider_chat_completion(
            &format!("{provider}/model"),
            "hello",
            "run-1",
            &mut output,
        );
        let Err(error) = result else {
            return Err(std::io::Error::other("credential-free provider call succeeded").into());
        };
        assert_eq!(
            error.message,
            format!("missing provider credential: {provider}")
        );
        assert_eq!(provider_request_attempts(), 0);
        return Ok(());
    }

    let root = unique_temp_dir("runner-brokered-provider-auth")?;
    let providers = root.join("providers.d");
    let provider = format!(
        "zztestegressauth{}{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    );
    let control = root.join(format!("model/{provider}/model.d"));
    fs::create_dir_all(&providers)?;
    fs::create_dir_all(&control)?;
    fs::write(
        providers.join(format!("{provider}.json")),
        format!(
            "{{\"name\":\"{provider}\",\"base_url\":\"https://api.example.test/v1\",\"formats\":[\"openai.chat\",\"openai.responses\"]}}\n"
        ),
    )?;
    fs::write(
        control.join("driver"),
        "default=openai-chat,openai-responses\n",
    )?;
    let status = std::process::Command::new(std::env::current_exe()?)
        .arg("brokered_external_provider_stops_all_openai_drivers_before_request")
        .arg("--nocapture")
        .env(CHILD_ENV, "1")
        .env(PROVIDER_ENV, &provider)
        .env("CTX_PROVIDER_CONFIG_DIR", &providers)
        .env("CTX_ROOT", &root)
        .env(
            cortexfs::runtime::egress::PROVIDER_EGRESS_DIR_ENV,
            cortexfs::runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH,
        )
        .env_remove("CTX_AGENT")
        .env_remove("CTX_PROVIDER_SECRET_VALUE")
        .env_remove("CTX_PROVIDER_SECRET_FD")
        .env_remove("CTX_PROVIDER_SECRET_PATH")
        .env_remove("CTX_PROVIDER_SECRET_PROVIDER")
        .env_remove("CTX_PROVIDER_SECRET_SLOT")
        .status()?;
    let _ignored = fs::remove_dir_all(root);

    assert!(
        status.success(),
        "brokered provider auth child assertion failed"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn agent_driver_route_falls_back_from_responses_to_chat() -> Result<(), Box<dyn std::error::Error>>
{
    const CHILD_ENV: &str = "CORTEXFS_TEST_AGENT_DRIVER_FALLBACK";
    if std::env::var_os(CHILD_ENV).is_some() {
        let mut output = Vec::new();
        runner::provider_chat_completion("fixture/model", "hello", "run-1", &mut output)
            .map_err(|error| std::io::Error::other(error.message))?;
        return Ok(());
    }

    let (base_url, stop, server) = spawn_driver_fallback_server()?;
    let root = unique_temp_dir("runner-agent-driver-fallback")?;
    let providers = root.join("providers.d");
    let control = root.join("model/fixture/model.d");
    fs::create_dir_all(&providers)?;
    fs::create_dir_all(&control)?;
    fs::write(
        providers.join("fixture.json"),
        format!(
            "{{\"name\":\"fixture\",\"base_url\":\"{base_url}/v1\",\"formats\":[\"openai.chat\",\"openai.responses\"]}}\n"
        ),
    )?;
    fs::write(
        control.join("driver"),
        "default=openai-chat\nagent=openai-responses,openai-chat\n",
    )?;
    let status = std::process::Command::new(std::env::current_exe()?)
        .arg("agent_driver_route_falls_back_from_responses_to_chat")
        .arg("--nocapture")
        .env(CHILD_ENV, "1")
        .env("CTX_PROVIDER_CONFIG_DIR", &providers)
        .env("CTX_ROOT", &root)
        .env("CTX_AGENT", "architect")
        .status()?;
    let _ignored = stop.send(());
    let paths = server
        .join()
        .map_err(|_panic| std::io::Error::other("provider test server panicked"))??;
    let _ignored = fs::remove_dir_all(root);

    assert!(status.success(), "driver fallback child assertion failed");
    assert_eq!(
        paths,
        ["/v1/responses", "/v1/responses", "/v1/chat/completions"]
    );
    Ok(())
}

#[cfg(unix)]
type DriverFallbackServer = (
    String,
    std::sync::mpsc::Sender<()>,
    thread::JoinHandle<std::io::Result<Vec<String>>>,
);

#[cfg(unix)]
fn spawn_driver_fallback_server() -> Result<DriverFallbackServer, Box<dyn std::error::Error>> {
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::mpsc::TryRecvError;

    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let (stop_sender, stop_receiver) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        let mut paths = Vec::new();
        loop {
            match stop_receiver.try_recv() {
                Ok(()) | Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }
            let (mut stream, _peer) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => return Err(error),
            };
            let mut request = [0_u8; 8 * 1024];
            let read = stream.read(&mut request)?;
            let head = String::from_utf8_lossy(request.get(..read).unwrap_or_default());
            let path = head
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or_default()
                .to_owned();
            paths.push(path);
            if paths.len() < 3 {
                stream.write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
            } else {
                let body = concat!(
                    r#"data: {"choices":[{"delta":{"content":"fallback"},"finish_reason":"stop"}]}"#,
                    "\n\n"
                );
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(header.as_bytes())?;
                stream.write_all(body.as_bytes())?;
            }
            stream.flush()?;
        }
        Ok(paths)
    });
    Ok((format!("http://{address}"), stop_sender, server))
}

#[cfg(unix)]
#[test]
fn provider_chat_finish_reason_is_terminal() -> Result<(), Box<dyn std::error::Error>> {
    let (result, requests, _output) = call_test_provider_sse(concat!(
        r#"data: {"choices":[{"delta":{"content":"complete"}}]}"#,
        "\n\n",
        r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        "\n\n"
    ))?;
    assert_eq!(requests, 1);
    if let Err(error) = result {
        return Err(std::io::Error::other(error.message).into());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn provider_chat_terminal_chunk_preserves_content() -> Result<(), Box<dyn std::error::Error>> {
    let (result, requests, output) = call_test_provider_sse(concat!(
        r#"data: {"choices":[{"delta":{"content":"complete"},"finish_reason":"stop"}]}"#,
        "\n\n"
    ))?;
    if let Err(error) = result {
        return Err(std::io::Error::other(error.message).into());
    }
    let frames = output
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(requests, 1);
    assert_eq!(
        frames,
        [serde_json::json!({
            "type": "delta",
            "run": "run-1",
            "text": "complete"
        })]
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn provider_chat_terminal_chunk_preserves_tool_call() -> Result<(), Box<dyn std::error::Error>> {
    let (result, requests, output) = call_test_provider_sse(concat!(
        r#"data: {"choices":[{"delta":{"tool_calls":[{"id":"call-abc","type":"function","function":{"name":"tsh","arguments":"{\"args\":[\"tools\"]}"}}]},"finish_reason":"tool_calls"}]}"#,
        "\n\n"
    ))?;
    if let Err(error) = result {
        return Err(std::io::Error::other(error.message).into());
    }
    let frames = output
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(requests, 1);
    assert_eq!(
        frames,
        [serde_json::json!({
            "type": "tool_call",
            "run": "run-1",
            "id": "call-abc",
            "name": "tsh",
            "arguments": { "args": ["tools"] }
        })]
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn provider_responses_completed_is_terminal() -> Result<(), Box<dyn std::error::Error>> {
    let (result, requests, _output) = call_test_provider_sse(concat!(
        r#"data: {"type":"response.output_text.delta","delta":"complete"}"#,
        "\n\n",
        r#"data: {"type":"response.completed"}"#,
        "\n\n"
    ))?;
    assert_eq!(requests, 1);
    if let Err(error) = result {
        return Err(std::io::Error::other(error.message).into());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn provider_done_marker_is_terminal() -> Result<(), Box<dyn std::error::Error>> {
    let (result, requests, _output) = call_test_provider_sse(concat!(
        r#"data: {"choices":[{"delta":{"content":"complete"}}]}"#,
        "\n\n",
        "data: [DONE]\n\n"
    ))?;
    assert_eq!(requests, 1);
    if let Err(error) = result {
        return Err(std::io::Error::other(error.message).into());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn provider_responses_text_done_without_completion_is_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let (result, requests, _output) = call_test_provider_sse(concat!(
        r#"data: {"type":"response.output_text.delta","delta":"partial"}"#,
        "\n\n",
        r#"data: {"type":"response.output_text.done","text":"partial"}"#,
        "\n\n"
    ))?;
    let Err(error) = result else {
        return Err(std::io::Error::other("non-terminal Responses stream succeeded").into());
    };
    assert_eq!(requests, 1);
    assert!(!error.can_fallback, "{}", error.message);
    Ok(())
}

#[cfg(unix)]
#[test]
fn provider_responses_rejects_chat_finish_reason_without_completion()
-> Result<(), Box<dyn std::error::Error>> {
    let (result, requests, _output) = call_test_provider_sse_at(
        "/v1/responses",
        runner::OpenAiStreamApi::Responses,
        concat!(
            r#"data: {"type":"response.output_text.delta","delta":"partial"}"#,
            "\n\n",
            r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            "\n\n"
        ),
    )?;
    let Err(error) = result else {
        return Err(std::io::Error::other("Chat finish_reason completed Responses stream").into());
    };
    assert_eq!(requests, 1);
    assert!(!error.can_fallback, "{}", error.message);
    Ok(())
}

#[cfg(unix)]
type ProviderSseOutcome = (Result<(), runner::StreamFailure>, usize, String);

#[cfg(unix)]
type ProviderSseServer = (
    String,
    std::sync::mpsc::Sender<()>,
    thread::JoinHandle<std::io::Result<usize>>,
);

#[cfg(unix)]
fn call_test_provider_sse(body: &str) -> Result<ProviderSseOutcome, Box<dyn std::error::Error>> {
    call_test_provider_sse_at("/v1/chat/completions", runner::OpenAiStreamApi::Chat, body)
}

#[cfg(unix)]
fn call_test_provider_sse_at(
    path: &str,
    api: runner::OpenAiStreamApi,
    body: &str,
) -> Result<ProviderSseOutcome, Box<dyn std::error::Error>> {
    let (base_url, stop, server) = spawn_provider_sse(body)?;
    let target = runner::CurlJsonTarget {
        url: format!("{base_url}{path}"),
        unix_socket: None,
    };
    let mut output = Vec::new();
    let result = runner::call_openai_sse_streaming(&target, &[], "{}", api, "run-1", &mut output);
    let _ignored = stop.send(());
    let requests = server
        .join()
        .map_err(|_panic| std::io::Error::other("provider test server panicked"))??;
    Ok((result, requests, String::from_utf8(output)?))
}

#[cfg(unix)]
fn spawn_provider_sse(first_body: &str) -> Result<ProviderSseServer, Box<dyn std::error::Error>> {
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::mpsc::TryRecvError;

    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let first_body = first_body.to_owned();
    let (stop_sender, stop_receiver) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        let mut requests = 0;
        loop {
            match stop_receiver.try_recv() {
                Ok(()) | Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }
            let (mut stream, _peer) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => return Err(error),
            };
            stream.set_read_timeout(Some(Duration::from_secs(1)))?;
            let mut request = [0_u8; 8 * 1024];
            let _read = stream.read(&mut request)?;
            requests += 1;
            let (content_type, body) = if requests == 1 {
                ("text/event-stream", first_body.as_str())
            } else {
                (
                    "application/json",
                    r#"{"choices":[{"message":{"content":"fallback"}}]}"#,
                )
            };
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes())?;
            stream.write_all(body.as_bytes())?;
            stream.flush()?;
        }
        Ok(requests)
    });
    Ok((format!("http://{address}"), stop_sender, server))
}
use super::runtime::test_prompt_context;
use super::*;
use crate::object::runner;
