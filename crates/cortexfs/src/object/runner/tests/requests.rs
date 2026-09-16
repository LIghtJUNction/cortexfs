use super::{openai_chat_body_with_agent_tools, openai_responses_body_with_agent_tools};
use serde_json::{Value, json};

#[test]
fn responses_agent_body_declares_tsh_function_tool() -> Result<(), Box<dyn std::error::Error>> {
    let effort = cortexfs::ModelEffort::Auto;
    for (body, function) in [
        (
            openai_responses_body_with_agent_tools("gpt-test", "hello", true, effort, true),
            "/tools/0",
        ),
        (
            openai_chat_body_with_agent_tools("gpt-test", "hello", true, effort, true),
            "/tools/0/function",
        ),
    ] {
        let value = serde_json::from_str::<Value>(&body)?;
        assert_eq!(value.pointer("/tools/0/type"), Some(&json!("function")));
        for (field, expected) in [
            ("name", json!("tsh")),
            ("parameters/properties/args/minItems", json!(1)),
            ("parameters/required", json!(["args"])),
            ("parameters/additionalProperties", json!(false)),
        ] {
            assert_eq!(
                value.pointer(&format!("{function}/{field}")),
                Some(&expected)
            );
        }
        assert_eq!(value.pointer(&format!("{function}/strict")), None);
        assert_eq!(value.get("tool_choice"), Some(&json!("auto")));
        assert_eq!(value.get("parallel_tool_calls"), Some(&json!(false)));
    }
    for (protocol, path) in [
        (cortexfs_protocol::WireProtocol::Anthropic, "/tools/0/name"),
        (
            cortexfs_protocol::WireProtocol::Gemini,
            "/tools/0/functionDeclarations/0/name",
        ),
    ] {
        let body = super::request_body(protocol, "model", "hello", false, effort, true);
        let value = serde_json::from_str::<Value>(&body)?;
        assert_eq!(value.pointer(path), Some(&json!("tsh")));
        assert_eq!(value.get("parallel_tool_calls"), None);
    }
    Ok(())
}
