use crate::agent::TOOL_CONTINUATION_CONTEXT_PREFIX;
use crate::object::runner::requests::agent_continuation_messages;
use crate::object::runner::responses::parse_anthropic_message_content;
use cortexfs_protocol::{Message, ModelRequest, ToolCall, WireProtocol, encode_model_request};
use serde_json::{Value, json};

#[test]
fn continuation_encodes_native_tool_results() -> Result<(), Box<dyn std::error::Error>> {
    let mut assistant = Message::assistant("");
    assistant.tool_calls.push(ToolCall {
        id: "call-1".to_owned(),
        name: "tsh".to_owned(),
        arguments: json!({"args": ["tools"]}),
    });
    let mut tool = Message::new("tool", "agent.\nfs.\n");
    tool.tool_call_id = Some("call-1".to_owned());
    let context = format!(
        "{TOOL_CONTINUATION_CONTEXT_PREFIX}{}",
        serde_json::to_string(&[assistant, tool])?
    );
    let messages = agent_continuation_messages(&context).ok_or("missing continuation")?;
    assert_eq!(messages[1].name.as_deref(), Some("tsh"));
    let responses = encoded(WireProtocol::OpenAiResponses, messages.clone())?;
    let chat = encoded(WireProtocol::OpenAiChat, messages.clone())?;
    let anthropic = encoded(WireProtocol::Anthropic, messages.clone())?;
    let gemini = encoded(WireProtocol::Gemini, messages)?;
    for (value, pointer, expected) in [
        (&responses, "/input/2/type", "function_call_output"),
        (&chat, "/messages/1/tool_call_id", "call-1"),
        (&anthropic, "/messages/1/role", "user"),
        (&anthropic, "/messages/1/content/0/type", "tool_result"),
        (&anthropic, "/messages/1/content/0/tool_use_id", "call-1"),
        (&anthropic, "/messages/1/content/0/content", "agent.\nfs.\n"),
        (&gemini, "/contents/0/parts/1/functionCall/name", "tsh"),
        (&gemini, "/contents/1/parts/0/functionResponse/id", "call-1"),
        (&gemini, "/contents/1/parts/0/functionResponse/name", "tsh"),
        (
            &gemini,
            "/contents/1/parts/0/functionResponse/response/content",
            "agent.\nfs.\n",
        ),
    ] {
        assert_eq!(value.pointer(pointer), Some(&json!(expected)));
    }
    assert!(chat.pointer("/messages/1/name").is_none());
    assert!(anthropic.pointer("/messages/1/content/1").is_none());
    assert!(gemini.pointer("/contents/1/parts/1").is_none());

    let input = br#"{"model":"model","contents":[{"role":"model","parts":[{"functionCall":{"id":"call-2","name":"tsh","args":{"args":["tools"]}}}]},{"role":"user","parts":[{"functionResponse":{"id":"call-2","name":"tsh","response":{"output":"ok"}}}]}]}"#;
    let decoded = cortexfs_protocol::decode_model_request(WireProtocol::Gemini, input)?;
    let decoded = serde_json::to_value(decoded)?;
    assert_eq!(decoded.pointer("/messages/0/tool_calls/0/id"), Some(&json!("call-2")));
    let direct =
        cortexfs_protocol::transcode_request(WireProtocol::Gemini, WireProtocol::OpenAiChat, input)?;
    let direct: Value = serde_json::from_slice(&direct.bytes)?;
    for pointer in ["/messages/0/tool_calls/0/id", "/messages/1/tool_call_id"] {
        assert_eq!(direct.pointer(pointer).and_then(Value::as_str), Some("call-2"));
    }
    Ok(())
}

#[test]
fn failed_provider_turn_wins_over_tool_call() {
    let response = br#"{"content":[{"type":"tool_use","id":"c","name":"tsh","input":{"args":["tools"]}}],"stop_reason":"error"}"#;
    assert_eq!(
        parse_anthropic_message_content(response),
        Err("provider response failed".to_owned())
    );
}

fn encoded(
    protocol: WireProtocol,
    messages: [Message; 2],
) -> Result<Value, Box<dyn std::error::Error>> {
    let bytes = encode_model_request(protocol, &ModelRequest::new("model", messages.into()))?;
    Ok(serde_json::from_slice(&bytes)?)
}
