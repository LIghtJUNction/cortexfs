use crate::agent::TOOL_CONTINUATION_CONTEXT_PREFIX;
use crate::object::runner::requests::agent_continuation_messages;
use cortexfs_protocol::{Message, ModelRequest, ToolCall, WireProtocol, encode_model_request};
use serde_json::{Value, json};

#[test]
fn continuation_encodes_native_openai_tool_result() -> Result<(), Box<dyn std::error::Error>> {
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
    assert_eq!(
        responses.pointer("/input/1/type"),
        Some(&json!("function_call"))
    );
    assert_eq!(
        responses.pointer("/input/1/call_id"),
        Some(&json!("call-1"))
    );
    assert_eq!(
        responses.pointer("/input/2/type"),
        Some(&json!("function_call_output"))
    );
    assert_eq!(
        responses.pointer("/input/2/call_id"),
        Some(&json!("call-1"))
    );

    let chat = encoded(WireProtocol::OpenAiChat, messages)?;
    assert_eq!(
        chat.pointer("/messages/0/tool_calls/0/function/name"),
        Some(&json!("tsh"))
    );
    assert_eq!(
        chat.pointer("/messages/1/tool_call_id"),
        Some(&json!("call-1"))
    );
    Ok(())
}

fn encoded(
    protocol: WireProtocol,
    messages: [Message; 2],
) -> Result<Value, Box<dyn std::error::Error>> {
    let bytes = encode_model_request(protocol, &ModelRequest::new("model", messages.into()))?;
    Ok(serde_json::from_slice(&bytes)?)
}
