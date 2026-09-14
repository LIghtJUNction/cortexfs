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
    let responses = encoded(WireProtocol::OpenAiResponses, messages.clone())?;
    assert_eq!(responses.pointer("/input/2/type"), Some(&json!("function_call_output")));
    assert_eq!(responses.pointer("/input/2/call_id"), Some(&json!("call-1")));
    let chat = encoded(WireProtocol::OpenAiChat, messages.clone())?;
    assert_eq!(chat.pointer("/messages/1/tool_call_id"), Some(&json!("call-1")));
    let anthropic = encoded(WireProtocol::Anthropic, messages.clone())?;
    assert_eq!(anthropic.pointer("/messages/1/content"), Some(&json!([{"type": "tool_result", "tool_use_id": "call-1", "content": "agent.\nfs.\n"}])));
    let gemini = encoded(WireProtocol::Gemini, messages)?;
    assert_eq!(gemini.pointer("/contents/1"), Some(&json!({"role": "user", "parts": [{"functionResponse": {"name": "call-1", "response": {"content": "agent.\nfs.\n"}}}]})));
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
