use crate::agent::TOOL_CONTINUATION_CONTEXT_PREFIX;
use crate::object::runner::requests::agent_continuation_messages;
use crate::object::runner::responses::parse_anthropic_message_content;
use cortexfs_protocol::{
    Message, ModelRequest, ToolCall, WireProtocol, decode_model_request, encode_model_request,
    transcode_request,
};
use serde_json::{Value, json};
use std::error::Error;

#[test]
fn continuation_encodes_native_tool_results() -> Result<(), Box<dyn Error>> {
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
        (&gemini, "/contents/1/role", "user"),
        (&gemini, "/contents/0/parts/1/functionCall/id", "call-1"),
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
    let input = br#"{"model":"model","contents":[{"role":"model","parts":[{"functionCall":{"id":"call-2","name":"tsh","args":{"args":["tools"]}}},{"functionCall":{"name":"legacy","args":{"args":[]}}}]},{"role":"user","parts":[{"functionResponse":{"id":"call-2","name":"tsh","response":{"output":"ok"}}},{"functionResponse":{"name":"legacy","response":{"output":"ok"}}}]}]}"#;
    let decoded = serde_json::to_string(&decode_model_request(WireProtocol::Gemini, input)?)?;
    assert!(decoded.contains(r#""id":"call-2""#) && decoded.contains(r#""id":"gemini-call-1""#));
    let direct = transcode_request(WireProtocol::Gemini, WireProtocol::OpenAiChat, input)?;
    let direct = String::from_utf8(direct.bytes)?;
    assert!(direct.contains(r#""id":"call-2""#) && direct.contains(r#""tool_call_id":"call-2""#));
    assert!(direct.contains(r#""id":"gemini-call-1""#) && direct.contains(r#""tool_call_id":"gemini-call-1""#));
    Ok(())
}

#[test]
fn unusable_provider_turns_fail_closed() {
    let response = br#"{"id":"r","model":"m","content":[{"type":"tool_use","id":"c","name":"tsh","input":{"args":["tools"]}}],"stop_reason":"max_tokens"}"#;
    let error = parse_anthropic_message_content(response).err();
    assert_eq!(error.as_deref(), Some("provider response failed"));
    for reason in ["cancelled", "error", "future_reason"] {
        let line = format!(r#"data: {{"choices":[{{"finish_reason":"{reason}"}}]}}"#);
        assert!(crate::object::runner::streaming::openai_stream_event(&line).is_err());
    }
}

fn encoded(protocol: WireProtocol, messages: [Message; 2]) -> Result<Value, Box<dyn Error>> {
    let bytes = encode_model_request(protocol, &ModelRequest::new("model", messages.into()))?;
    Ok(serde_json::from_slice(&bytes)?)
}
