use crate::object::runner::responses::parse_provider_content as parse;
use cortexfs_protocol::WireProtocol;
use serde_json::{Value, json};

#[test]
fn responses_agent_body_declares_tsh_function_tool() -> Result<(), Box<dyn std::error::Error>> {
    let effort = cortexfs::ModelEffort::Auto;
    for (protocol, function) in [
        (WireProtocol::OpenAiResponses, "/tools/0"),
        (WireProtocol::OpenAiChat, "/tools/0/function"),
    ] {
        let body = super::request_body(protocol, "gpt-test", "hello", true, effort, true);
        let value = serde_json::from_str::<Value>(&body)?;
        assert_eq!(value.pointer("/tools/0/type"), Some(&json!("function")));
        for (field, expected) in [
            ("name", json!("tsh")),
            ("parameters/properties/args/minItems", json!(1)),
            ("parameters/required", json!(["args"])),
            ("parameters/additionalProperties", json!(false)),
        ] {
            let pointer = format!("{function}/{field}");
            assert_eq!(value.pointer(&pointer), Some(&expected));
        }
        assert_eq!(value.pointer(&format!("{function}/strict")), None);
        assert_eq!(value.get("tool_choice"), Some(&json!("auto")));
        assert_eq!(value.get("parallel_tool_calls"), Some(&json!(false)));
    }
    for (protocol, path) in [
        (WireProtocol::Anthropic, "/tools/0/name"),
        (WireProtocol::Gemini, "/tools/0/functionDeclarations/0/name"),
    ] {
        let body = super::request_body(protocol, "model", "hello", false, effort, true);
        let value = serde_json::from_str::<Value>(&body)?;
        assert_eq!(value.pointer(path), Some(&json!("tsh")));
        assert_eq!(value.get("parallel_tool_calls"), None);
    }
    Ok(())
}

#[test]
fn responses_runner_filters_provider_content() {
    for status in ["queued", "in_progress", "future_status"] {
        let body = json!({"status":status,"output_text":"ignored"}).to_string();
        let actual = parse(WireProtocol::OpenAiResponses, body.as_bytes()).err();
        assert_eq!(actual, Some(format!("provider response {status}")));
    }
    let b = br#"{"content":[{"type":"thinking","thinking":"x"},{"type":"text","text":"public"}]}"#;
    assert_eq!(parse(WireProtocol::Anthropic, b), Ok("public".into()));
}
