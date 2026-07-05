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
    let event = openai_stream_event(
        r#"data: {"choices":[{"delta":{"content":"hel"}}]}"#,
    );
    assert!(matches!(event, Ok(OpenAiStreamEvent::Delta(text)) if text == "hel"));

    let event = openai_stream_event(
        r#"data: {"choices":[{"delta":{"tool_calls":[{"id":"call-abc","type":"function","function":{"name":"tsh","arguments":"{\"args\""}}]}}]}"#,
    );
    assert!(matches!(
        event,
        Ok(OpenAiStreamEvent::ToolCallDelta(delta))
            if delta.id.as_deref() == Some("call-abc")
                && delta.name.as_deref() == Some("tsh")
                && delta.arguments == r#"{"args""#
    ));

    assert!(matches!(
        openai_stream_event(r#"data: {"choices":[{"finish_reason":"tool_calls"}]}"#),
        Ok(OpenAiStreamEvent::ToolCallsDone)
    ));
}

#[test]
fn openai_stream_tool_call_stream_accumulates_canonical_tool_call()
-> Result<(), Box<dyn std::error::Error>> {
    let mut stream = OpenAiToolCallStream::default();
    let first = openai_stream_event(
        r#"data: {"choices":[{"delta":{"tool_calls":[{"id":"call-abc","type":"function","function":{"name":"tsh","arguments":"{\"args\""}}]}}]}"#,
    )?;
    let second = openai_stream_event(
        r#"data: {"choices":[{"delta":{"tool_calls":[{"type":"function","function":{"arguments":":[\"tools\"]}"}}]}}]}"#,
    )?;

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
    let event = openai_stream_event(
        r#"data: {"usage":{"prompt_tokens":12,"completion_tokens":5}}"#,
    );
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
        openai_stream_event("data: [DONE]"),
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
    let event = openai_stream_event(
        r#"data: {"type":"response.output_text.delta","delta":"hel"}"#,
    );
    assert!(matches!(event, Ok(OpenAiStreamEvent::Delta(text)) if text == "hel"));
}

#[test]
fn openai_stream_event_extracts_responses_done_text() {
    let event = openai_stream_event(
        r#"data: {"type":"response.output_text.done","text":"hel"}"#,
    );
    assert!(matches!(event, Ok(OpenAiStreamEvent::FinalText(text)) if text == "hel"));

    let event = openai_stream_event(
        r#"data: {"type":"response.content_part.done","part":{"type":"output_text","text":"lo"}}"#,
    );
    assert!(matches!(event, Ok(OpenAiStreamEvent::FinalText(text)) if text == "lo"));

    let event = openai_stream_event(
        r#"data: {"type":"response.output_item.done","item":{"content":[{"type":"output_text","text":"ok"}]}}"#,
    );
    assert!(matches!(event, Ok(OpenAiStreamEvent::FinalText(text)) if text == "ok"));
}

#[test]
fn responses_done_text_is_not_treated_as_stream_delta() -> Result<(), Box<dyn std::error::Error>> {
    let events = [
        openai_stream_event(r#"data: {"type":"response.output_text.delta","delta":"Hi!"}"#),
        openai_stream_event(r#"data: {"type":"response.output_text.done","text":"Hi!"}"#),
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
        openai_stream_event(r#"data: {"type":"response.completed"}"#),
        Ok(OpenAiStreamEvent::Done)
    ));
}

#[test]
fn openai_stream_event_reports_responses_failed_marker() {
    assert_eq!(
        openai_stream_event(
            r#"data: {"type":"response.failed","error":{"message":"quota exceeded"}}"#
        )
        .map(|event| matches!(event, OpenAiStreamEvent::Ignore)),
        Err("quota exceeded".to_owned())
    );
}

#[test]
fn openai_stream_event_does_not_mix_reasoning_into_answer_text() {
    let event = openai_stream_event(
        r#"data: {"choices":[{"delta":{"reasoning_content":"hidden"}}]}"#,
    );
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
    let chat = openai_chat_body("gpt-5.5", "hello", false, cortexfs::ModelEffort::High);
    let responses =
        openai_responses_body("gpt-5.5", "hello", true, cortexfs::ModelEffort::XHigh);

    assert!(chat.contains(r#""reasoning":{"effort":"high"}"#));
    assert!(responses.contains(r#""reasoning":{"effort":"xhigh"}"#));
    assert!(!openai_chat_body("gpt-5.5", "hello", false, cortexfs::ModelEffort::Auto)
        .contains("reasoning"));
}

#[test]
fn openai_agent_chat_body_declares_tsh_tool() {
    let body = openai_chat_body_with_agent_tools(
        "gpt-5.5",
        "what tools?",
        true,
        cortexfs::ModelEffort::Auto,
        true,
    );
    assert!(body.contains(r#""name":"tsh""#), "{body}");
    assert!(body.contains(r#""tool_choice":"auto""#), "{body}");
    assert!(body.contains(r#""stream_options":{"include_usage":true}"#), "{body}");
    assert!(body.contains(r#""required":["args"]"#), "{body}");
}

#[test]
fn openai_chat_tool_call_response_parses_as_canonical_tool_call() {
    let content = parse_openai_chat_content(
        br#"{"choices":[{"message":{"tool_calls":[{"id":"call-abc","type":"function","function":{"name":"tsh","arguments":"{\"args\":[\"tools\"]}"}}]}}]}"#,
    );
    assert_eq!(
        content,
        Ok(r#"{"arguments":{"args":["tools"]},"id":"call-abc","name":"tsh","type":"tool_call"}"#.to_owned())
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
    assert!(system.contains("only native callable tool exposed by this runtime is `tsh`"));
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
        messages.pointer("/1/content").and_then(serde_json::Value::as_str),
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
    context.template = "agent={{agent}}\ninstructions={{agent_instructions}}\ncontract={{runtime_contract}}\n".to_owned();

    let prompt = render_agent_system_prompt("coder", "custom identity", &context);

    assert!(prompt.starts_with("agent=coder\ninstructions=custom identity\n"));
    assert!(prompt.contains("contract=You are CortexFS agent `coder`."));
    assert!(!prompt.contains("## Rules"));
}
