#[test]
fn provider_text_tool_call_writes_canonical_event() {
    let mut output = Vec::new();
    let result = write_model_text_or_tool_call(
        &mut output,
        "run-1",
        r#"{"type":"tool_call","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}I will use the result next."#,
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
fn openai_stream_tool_call_stream_accumulates_canonical_tool_call()
-> Result<(), Box<dyn std::error::Error>> {
    let mut stream = OpenAiToolCallStream::default();
    let OpenAiStreamEvent::ToolCallDelta(delta) = openai_stream_event(
        r#"data: {"choices":[{"delta":{"tool_calls":[{"id":"call-abc","type":"function","function":{"name":"tsh","arguments":"{\"args\""}}]}}]}"#,
    )?
    .event else {
        return Err("expected tool call delta".into());
    };
    assert_eq!(delta.id.as_deref(), Some("call-abc"));
    assert_eq!(delta.name.as_deref(), Some("tsh"));
    assert_eq!(delta.arguments, r#"{"args""#);
    stream.push(delta);
    if let OpenAiStreamEvent::ToolCallDelta(delta) = openai_stream_event(
        r#"data: {"choices":[{"delta":{"tool_calls":[{"type":"function","function":{"arguments":":[\"tools\"]}"}}]}}]}"#,
    )?
    .event {
        stream.push(delta);
    }

    assert_eq!(
        stream.finish()?,
        Some(
            r#"{"arguments":{"args":["tools"]},"id":"call-abc","name":"tsh","type":"tool_call"}"#
                .to_owned()
        )
    );
    for id in ["", r#""id":"call/invalid","#] {
        let line = format!(
            r#"data: {{"choices":[{{"delta":{{"tool_calls":[{{{id}"function":{{"name":"tsh","arguments":"{{\"args\":[\"tools\"]}}"}}}}]}}}}]}}"#
        );
        let OpenAiStreamEvent::ToolCallDelta(delta) = openai_stream_event(&line)?.event else {
            return Err("expected tool call delta".into());
        };
        let mut invalid = OpenAiToolCallStream::default();
        invalid.push(delta);
        assert!(invalid.finish().is_err());
    }
    Ok(())
}

#[test]
fn openai_stream_tool_call_ignores_extra_provider_indices() -> Result<(), Box<dyn std::error::Error>>
{
    let mut stream = OpenAiToolCallStream::default();
    for line in [
        r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-first","function":{"name":"tsh","arguments":"{\"args\":[\"pwd\"]}"}}]}}]}"#,
        r#"data: {"choices":[{"delta":{"tool_calls":[{"index":1,"id":"call-second","function":{"name":"tsh","arguments":"{\"args\":[\"ls\"]}"}}]}}]}"#,
    ] {
        let OpenAiStreamEvent::ToolCallDelta(delta) = openai_stream_event(line)?.event else {
            return Err("expected tool call delta".into());
        };
        stream.push(delta);
    }
    assert_eq!(
        stream.finish()?,
        Some(
            r#"{"arguments":{"args":["pwd"]},"id":"call-first","name":"tsh","type":"tool_call"}"#
                .to_owned()
        )
    );
    Ok(())
}

#[test]
fn model_usage_preserves_optional_cache_metrics() -> Result<(), Box<dyn std::error::Error>> {
    let usage = |cached_tokens, cache_write_tokens| TokenUsage {
        input_tokens: 12,
        output_tokens: 5,
        cached_tokens,
        cache_write_tokens,
    };
    for (usage, expected) in [
        (
            usage(None, None),
            r#"{"type":"usage","run":"run-1","input_tokens":12,"output_tokens":5}
"#,
        ),
        (
            usage(Some(8), Some(4)),
            r#"{"type":"usage","run":"run-1","input_tokens":12,"output_tokens":5,"cached_tokens":8,"cache_write_tokens":4}
"#,
        ),
    ] {
        let mut output = Vec::new();
        write_model_usage(&mut output, "run-1", usage)?;
        assert_eq!(String::from_utf8(output)?, expected);
    }
    Ok(())
}

#[test]
fn responses_stream_function_calls_remain_direct_or_fail_closed() -> Result<(), String> {
    let response = parse_openai_response_content(br#"{"output_text":"hello codex"}"#);
    assert_eq!(response, Ok("hello codex".to_owned()));
    for response in [br"{}".as_slice(), br#"{"output":{}}"#.as_slice()] {
        assert_eq!(
            parse_openai_response_content(response).err().as_deref(),
            Some("provider response missing content")
        );
    }
    assert_eq!(
        parse_openai_response_content(
            br#"{"output_text":"explanation","output":[{"type":"function_call","call_id":"call_123","name":"tsh","arguments":"{\"args\":[\"tools\"]}","caller":{"type":"direct"}}]}"#,
        ),
        Ok(r#"{"arguments":{"args":["tools"]},"id":"call_123","name":"tsh","type":"tool_call"}"#.to_owned())
    );
    let direct = r#"{"type":"response.output_item.added","item":{"type":"function_call","call_id":"call_123","name":"tsh","arguments":"","caller":{"type":"direct"}}}"#;
    assert!(openai_stream_event(&format!("data: {direct}")).is_ok());
    let OpenAiStreamEvent::ToolCall(frame) = openai_stream_event(
        r#"data: {"type":"response.output_item.done","item":{"type":"function_call","call_id":"call_123","name":"tsh","arguments":"{\"args\":[\"tools\"]}","caller":{"type":"direct"}}}"#,
    )?.event else { return Err("expected tool call event".to_owned()) };
    assert_eq!(
        frame,
        r#"{"arguments":{"args":["tools"]},"id":"call_123","name":"tsh","type":"tool_call"}"#
    );
    for event in ["added", "done"] {
        for item in [
            serde_json::json!({"type":"program"}),
            serde_json::json!({"type":"program_output"}),
            serde_json::json!({"type":"function_call","caller":{"type":"program","caller_id":"prog_123"}}),
            serde_json::json!({"type":"function_call","caller":{"type":"unknown"}}),
        ] {
            let line =
                serde_json::json!({"type":format!("response.output_item.{event}"),"item":item});
            assert_eq!(
                openai_stream_event(&format!("data: {line}"))
                    .err()
                    .as_deref(),
                Some("provider response requires host-owned program continuation")
            );
        }
    }
    for (line, expected) in [
        (
            r#"data: {"type":"response.failed","response":{"error":{"message":"quota exceeded"}}}"#,
            "quota exceeded",
        ),
        (
            r#"data: {"type":"response.failed","error":{"message":"legacy quota"}}"#,
            "legacy quota",
        ),
        (
            r#"data: {"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}},"error":{"message":"incomplete fallback"}}"#,
            "max_output_tokens",
        ),
        (
            r#"data: {"type":"error","error":{"message":"generic fallback"},"message":"ignored"}"#,
            "generic fallback",
        ),
    ] {
        assert_eq!(openai_stream_event(line).err().as_deref(), Some(expected));
    }
    Ok(())
}

#[test]
fn openai_response_content_parses_output_parts() {
    for (response, expected) in [
        (br#"{"output":[{"content":[{"type":"output_text","text":"hello "},{"type":"output_text","text":"codex"}]}]}"#.as_slice(), Ok("hello codex")),
        (br#"{"output":[{"content":[{"type":"refusal","refusal":"cannot comply"}]}]}"#.as_slice(), Ok("cannot comply")),
        (br#"{"status":"failed","error":{"message":"quota"},"output":[{"type":"function_call","call_id":"call_123","name":"tsh","arguments":"{\"args\":[]}"}]}"#.as_slice(), Err("quota")),
        (br#"{"status":"cancelled","output_text":"ignored"}"#.as_slice(), Err("provider response cancelled")),
        (br#"{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"output":[{"content":[{"type":"output_text","text":"ignored"}]}]}"#.as_slice(), Err("max_output_tokens")),
        (br#"{"status":"failed","output_text":"ignored"}"#.as_slice(), Err("provider response failed")),
        (br#"{"status":"incomplete","output_text":"ignored"}"#.as_slice(), Err("provider response incomplete")),
    ] {
        assert_eq!(parse_openai_response_content(response), expected.map(str::to_owned).map_err(str::to_owned));
    }
}

#[test]
fn responses_stream_text_frames_preserve_terminal_events() -> Result<(), String> {
    for (line, kind, text) in [
        (
            r#"data: {"choices":[{"delta":{"content":"hel"}}]}"#,
            "delta",
            "hel",
        ),
        (
            r#"data: {"choices":[{"finish_reason":"tool_calls"}]}"#,
            "done",
            "",
        ),
        ("data: [DONE]", "done", ""),
        (
            r#"data: {"type":"response.output_text.delta","delta":"hel"}"#,
            "delta",
            "hel",
        ),
        (
            r#"data: {"type":"response.output_text.done","text":"hel"}"#,
            "final",
            "hel",
        ),
        (
            r#"data: {"type":"response.refusal.delta","delta":"cannot "}"#,
            "delta",
            "cannot ",
        ),
        (
            r#"data: {"type":"response.refusal.done","refusal":"comply"}"#,
            "final",
            "comply",
        ),
        (
            r#"data: {"type":"response.content_part.done","part":{"type":"output_text","text":"lo"}}"#,
            "final",
            "lo",
        ),
        (
            r#"data: {"type":"response.content_part.done","part":{"type":"refusal","refusal":"no"}}"#,
            "final",
            "no",
        ),
        (
            r#"data: {"type":"response.output_item.done","item":{"content":[{"type":"output_text","text":"ok"}]}}"#,
            "final",
            "ok",
        ),
        (r#"data: {"type":"response.completed"}"#, "completed", ""),
        (
            r#"data: {"type":"response.completed","response":{"usage":{"input_tokens":"invalid"}}}"#,
            "completed",
            "",
        ),
        (r#"data: {"type":"response.done"}"#, "response_done", ""),
        (
            r#"data: {"type":"response.function_call_arguments.delta","delta":"{}"}"#,
            "ignore",
            "",
        ),
        (
            r#"data: {"choices":[{"delta":{"reasoning_content":"hidden"}}]}"#,
            "delta",
            "",
        ),
    ] {
        let event = openai_stream_event(line)?.event;
        let actual = match event {
            OpenAiStreamEvent::Delta(value) => ("delta", value),
            OpenAiStreamEvent::FinalText(value) => ("final", value),
            OpenAiStreamEvent::ResponseCompleted(_) => ("completed", String::new()),
            OpenAiStreamEvent::ResponseDone => ("response_done", String::new()),
            OpenAiStreamEvent::Done => ("done", String::new()),
            OpenAiStreamEvent::Ignore => ("ignore", String::new()),
            _ => return Err("unexpected stream event".to_owned()),
        };
        assert_eq!(actual, (kind, text.to_owned()));
    }
    assert!(
        read_provider_stream_line(&mut Cursor::new(
            format!("{}\n", "x".repeat(MAX_PROVIDER_STREAM_LINE_BYTES)).into_bytes(),
        ))
        .is_err()
    );
    Ok(())
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

    assert_eq!(String::from_utf8(output)?.matches("Hi!").count(), 1);
    Ok(())
}

#[test]
fn provider_usage_accepts_openai_and_anthropic_shapes() {
    let usage = |input_tokens, output_tokens, cached_tokens, cache_write_tokens| TokenUsage {
        input_tokens,
        output_tokens,
        cached_tokens,
        cache_write_tokens,
    };
    for (value, expected) in [
        (
            serde_json::json!({"usage":{"prompt_tokens":10,"completion_tokens":3}}),
            usage(10, 3, None, None),
        ),
        (
            serde_json::json!({"usage":{"input_tokens":7,"output_tokens":2}}),
            usage(7, 2, None, None),
        ),
        (
            serde_json::json!({"response":{"usage":{"input_tokens":9,"output_tokens":4}}}),
            usage(9, 4, None, None),
        ),
        (
            serde_json::json!({"usage":{"input_tokens":11,"output_tokens":5,"input_tokens_details":{"cached_tokens":8,"cache_write_tokens":4}}}),
            usage(11, 5, Some(8), Some(4)),
        ),
        (
            serde_json::json!({"usage":{"prompt_tokens":11,"completion_tokens":5,"prompt_tokens_details":{"cached_tokens":8,"cache_write_tokens":4}}}),
            usage(11, 5, Some(8), Some(4)),
        ),
    ] {
        assert_eq!(token_usage_from_value(&value), Some(expected));
    }
    assert!(matches!(
        openai_stream_event(r#"data: {"usage":{"prompt_tokens":12,"completion_tokens":5}}"#)
            .map(|frame| frame.event),
        Ok(OpenAiStreamEvent::Usage(TokenUsage {
            input_tokens: 12,
            output_tokens: 5,
            cached_tokens: None,
            cache_write_tokens: None
        }))
    ));
}

#[test]
fn openai_request_bodies_include_effort_and_agent_tools() {
    let chat = openai_chat_body("gpt-5.6", "hello", false, cortexfs::ModelEffort::High);
    let responses = openai_responses_body("gpt-5.6", "hello", true, cortexfs::ModelEffort::XHigh);

    assert!(chat.contains(r#""reasoning_effort":"high""#));
    assert!(!chat.contains(r#""reasoning":"#));
    assert!(responses.contains(r#""reasoning":{"effort":"xhigh"}"#));
    assert!(!responses.contains("reasoning_effort"));
    let chat_none = openai_chat_body("gpt-5.6", "hello", false, cortexfs::ModelEffort::None);
    assert!(chat_none.contains(r#""reasoning_effort":"none""#));
    assert!(!chat_none.contains(r#""reasoning":"#));
    assert!(
        openai_responses_body("gpt-5.6", "hello", false, cortexfs::ModelEffort::None)
            .contains(r#""reasoning":{"effort":"none"}"#)
    );
    assert!(
        openai_responses_body("gpt-5.6", "hello", false, cortexfs::ModelEffort::Max)
            .contains(r#""reasoning":{"effort":"max"}"#)
    );
    let chat_auto = openai_chat_body("gpt-5.6", "hello", false, cortexfs::ModelEffort::Auto);
    assert!(!chat_auto.contains("reasoning"));
    let responses_auto =
        openai_responses_body("gpt-5.6", "hello", false, cortexfs::ModelEffort::Auto);
    assert!(!responses_auto.contains("reasoning"));
    let body = openai_chat_body_with_agent_tools(
        "gpt-5.6",
        "what tools?",
        true,
        cortexfs::ModelEffort::Auto,
        true,
    );
    assert!(body.contains(r#""name":"tsh""#), "{body}");
    assert!(body.contains(r#""tool_choice":"auto""#), "{body}");
    assert!(body.contains(r#""parallel_tool_calls":false"#), "{body}");
    assert!(
        body.contains(r#""stream_options":{"include_usage":true}"#),
        "{body}"
    );
    assert!(body.contains(r#""required":["args"]"#), "{body}");
}

#[test]
fn openai_chat_tool_call_response_parses_as_canonical_tool_call() {
    let content = parse_openai_chat_content(
        br#"{"choices":[{"message":{"content":"explanation","tool_calls":[{"id":"call-abc","type":"function","function":{"name":"tsh","arguments":"{\"args\":[\"tools\"]}"}}]}}]}"#,
    );
    assert_eq!(
        content,
        Ok(
            r#"{"arguments":{"args":["tools"]},"id":"call-abc","name":"tsh","type":"tool_call"}"#
                .to_owned()
        )
    );
    for (reason, tool) in [("length", false), ("content_filter", true)] {
        let content = if tool {
            r#"{"tool_calls":[{"id":"call-abc","type":"function","function":{"name":"tsh","arguments":"{\"args\":[\"tools\"]}"}}]}"#
        } else {
            r#"{"content":"partial"}"#
        };
        let body = format!(r#"{{"choices":[{{"message":{content},"finish_reason":"{reason}"}}]}}"#);
        assert_eq!(
            parse_openai_chat_content(body.as_bytes()).err().as_deref(),
            Some(format!("provider response finished with {reason}").as_str())
        );
        let line =
            format!(r#"data: {{"choices":[{{"delta":{content},"finish_reason":"{reason}"}}]}}"#);
        assert_eq!(
            openai_stream_event(&line).err().as_deref(),
            Some(format!("provider response finished with {reason}").as_str())
        );
    }
}

#[test]
fn agent_provider_messages_expose_only_tsh_as_native_tool() {
    let messages = provider_messages_for_agent(
        "what tools?",
        Some("executor"),
        "Always answer tersely.",
        &test_prompt_context(),
    );
    let system = messages
        .pointer("/0/content")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(system.contains("## AGENT Instructions"));
    assert!(system.contains("Always answer tersely."));
    assert!(system.contains("Project rule"));
    assert!(system.contains("name: rust"));
    assert!(system.contains("tool output"));
    assert!(system.contains(r#"call `tsh` with `{"args":["..."]}`"#));
    assert!(system.contains("Inspect and report for answer, explain, review, diagnose, or plan"));
    assert!(system.contains("Require confirmation for external writes, destructive actions, or material scope expansion."));
    assert!(system.contains("Answer concisely: lead with the outcome, retain required evidence, caveats, and next action, and omit repetition."));
    assert!(system.contains("previous message"));
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

    let prompt = render_agent_system_prompt("executor", "", &test_prompt_context());
    assert!(prompt.contains("CortexFS agent `executor`"));
    assert!(prompt.contains(
        "Do not claim provider, host, assistant-platform, or hidden-platform access, including `image_gen`"
    ));
}

#[test]
fn agent_prompt_context_renders_workspace_runtime_hint() {
    let mut prompt_context = test_prompt_context();
    prompt_context.tool_injection = default_agent_tool_context();
    let prompt = render_agent_system_prompt("executor", "", &prompt_context);

    let expected = "\
Runtime workspace:
- `/workspace` is the agent's project workspace when mounted.
- `CTX_SOURCE` points at the CortexFS source view that defines visible agents, tools, models, sessions, and policy.
- `CTX_ROOT` points at the mounted CortexFS ABI root.
- No tool facts, repo structure, search results, or file content have been injected yet.";
    let tool_context = default_agent_tool_context();
    assert_eq!(tool_context, expected);
    assert!(prompt.contains(expected));
}

#[test]
fn agent_prompt_template_controls_rendered_system_message() {
    let mut context = test_prompt_context();
    context.template =
        "agent={{agent}}\ninstructions={{agent_instructions}}\ncontract={{runtime_contract}}\n"
            .to_owned();

    let prompt = render_agent_system_prompt("executor", "custom identity", &context);

    assert!(prompt.starts_with("agent=executor\ninstructions=custom identity\n"));
    assert!(prompt.contains("contract=You are CortexFS agent `executor`."));
    assert!(!prompt.contains("## Rules"));
}

#[cfg(unix)]
#[test]
fn provider_empty_reply_retries_transport_request() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::mpsc::TryRecvError;

    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
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
            if requests == 1 {
                continue;
            }
            let body = r#"{"ok":true}"#;
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes())?;
            stream.write_all(body.as_bytes())?;
            stream.flush()?;
            break;
        }
        Ok(requests)
    });
    let target = runner::CurlJsonTarget {
        url: format!("http://{address}"),
        unix_socket: None,
    };
    let response = runner::run_curl_json_with_headers(&target, &[], "{}")?;
    let _ignored = stop_sender.send(());
    let requests = server
        .join()
        .map_err(|_panic| std::io::Error::other("provider retry server panicked"))??;

    assert_eq!(requests, 2);
    assert_eq!(response, br#"{"ok":true}"#);
    Ok(())
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
fn provider_terminal_markers_preserve_payloads() -> Result<(), Box<dyn std::error::Error>> {
    for (api, body, expected) in [
        (
            runner::OpenAiStreamApi::Chat,
            concat!(
                r#"data: {"choices":[{"delta":{"content":"complete"}}]}"#,
                "\n\n",
                r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
                "\n\n"
            ),
            "{\"type\":\"delta\",\"run\":\"run-1\",\"text\":\"complete\"}\n",
        ),
        (
            runner::OpenAiStreamApi::Chat,
            concat!(
                r#"data: {"choices":[{"delta":{"content":"complete"},"finish_reason":"stop"}]}"#,
                "\n\n"
            ),
            "{\"type\":\"delta\",\"run\":\"run-1\",\"text\":\"complete\"}\n",
        ),
        (
            runner::OpenAiStreamApi::Chat,
            concat!(
                r#"data: {"choices":[{"delta":{"tool_calls":[{"id":"call-abc","type":"function","function":{"name":"tsh","arguments":"{\"args\":[\"tools\"]}"}}]},"finish_reason":"tool_calls"}]}"#,
                "\n\n"
            ),
            r#"{"arguments":{"args":["tools"]},"id":"call-abc","name":"tsh","run":"run-1","type":"tool_call"}
"#,
        ),
        (
            runner::OpenAiStreamApi::Responses,
            r#"data: {"type":"response.output_item.done","item":{"type":"function_call","call_id":"call-abc","name":"tsh","arguments":"{\"args\":[\"tools\"]}"}}

data: {"type":"response.completed","response":{"usage":{"input_tokens":12,"output_tokens":5,"input_tokens_details":{"cached_tokens":8,"cache_write_tokens":4}}}}

"#,
            r#"{"arguments":{"args":["tools"]},"id":"call-abc","name":"tsh","run":"run-1","type":"tool_call"}
{"type":"usage","run":"run-1","input_tokens":12,"output_tokens":5,"cached_tokens":8,"cache_write_tokens":4}
"#,
        ),
        (
            runner::OpenAiStreamApi::Responses,
            r#"data: {"type":"response.output_text.delta","delta":"plan"}

data: {"type":"response.output_item.done","item":{"type":"function_call","call_id":"call-abc","name":"tsh","arguments":"{\"args\":[\"tools\"]}"}}

data: {"type":"response.completed","response":{"usage":{"input_tokens":12,"output_tokens":5}}}

"#,
            r#"{"type":"delta","run":"run-1","text":"plan"}
{"arguments":{"args":["tools"]},"id":"call-abc","name":"tsh","run":"run-1","type":"tool_call"}
{"type":"usage","run":"run-1","input_tokens":12,"output_tokens":5}
"#,
        ),
        (
            runner::OpenAiStreamApi::Responses,
            r#"data: {"type":"response.output_item.done","item":{"type":"function_call","call_id":"call-abc","name":"tsh","arguments":"{\"args\":[\"tools\"]}"}}

data: {"type":"response.completed"}

"#,
            r#"{"arguments":{"args":["tools"]},"id":"call-abc","name":"tsh","run":"run-1","type":"tool_call"}
"#,
        ),
        (
            runner::OpenAiStreamApi::Chat,
            concat!(
                r#"data: {"choices":[{"delta":{"content":"complete"}}]}"#,
                "\n\n",
                "data: [DONE]\n\n"
            ),
            "{\"type\":\"delta\",\"run\":\"run-1\",\"text\":\"complete\"}\n",
        ),
    ] {
        let path = if api == runner::OpenAiStreamApi::Chat {
            "/v1/chat/completions"
        } else {
            "/v1/responses"
        };
        let (result, requests, output) = call_test_provider_sse_at(path, api, body)?;
        assert_eq!(requests, 1);
        result.map_err(|error| std::io::Error::other(error.message))?;
        assert_eq!(output, expected);
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn provider_responses_refusal_stream_emits_one_text() -> Result<(), Box<dyn std::error::Error>> {
    let body = r#"data: {"type":"response.refusal.delta","delta":"cannot comply"}

data: {"type":"response.refusal.done","refusal":"cannot comply"}

data: {"type":"response.content_part.done","part":{"type":"refusal","refusal":"cannot comply"}}

data: {"type":"response.output_item.done","item":{"content":[{"type":"refusal","refusal":"cannot comply"}]}}

data: {"type":"response.completed"}

"#;
    let (result, requests, output) =
        call_test_provider_sse_at("/v1/responses", runner::OpenAiStreamApi::Responses, body)?;
    assert_eq!(requests, 1);
    result.map_err(|error| std::io::Error::other(error.message))?;
    assert_eq!(
        output,
        "{\"type\":\"delta\",\"run\":\"run-1\",\"text\":\"cannot comply\"}\n"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn provider_responses_pending_calls_require_completion() -> Result<(), Box<dyn std::error::Error>> {
    let call = r#"data: {"type":"response.output_item.done","item":{"type":"function_call","call_id":"call-abc","name":"tsh","arguments":"{\"args\":[\"tools\"]}"}}"#;
    for suffix in [
        r#"data: {"type":"response.done"}"#,
        r#"data: {"type":"response.failed","error":{"message":"quota"}}"#,
        r#"data: {"type":"response.incomplete","response":{"incomplete_details":{"reason":"limit"}}}"#,
        r#"data: {"type":"error","error":{"message":"failure"}}"#,
        "",
        call,
        concat!(
            r#"data: {"type":"response.done"}"#,
            "\n\n",
            r#"data: {"type":"response.completed"}"#
        ),
    ] {
        let body = format!("{call}\n\n{suffix}\n\n");
        let (result, requests, output) =
            call_test_provider_sse_at("/v1/responses", runner::OpenAiStreamApi::Responses, &body)?;
        let Err(error) = result else {
            return Err(std::io::Error::other("non-terminal Responses stream succeeded").into());
        };
        assert_eq!(requests, 1);
        assert!(error.can_fallback, "{}", error.message);
        assert!(output.is_empty());
    }
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
