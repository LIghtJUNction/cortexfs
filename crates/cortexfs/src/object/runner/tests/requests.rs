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
            ("strict", json!(true)),
        ] {
            assert_eq!(
                value.pointer(&format!("{function}/{field}")),
                Some(&expected)
            );
        }
        assert_eq!(value.get("tool_choice"), Some(&json!("auto")));
        assert_eq!(value.get("parallel_tool_calls"), Some(&json!(false)));
        assert_eq!(
            value.pointer(&format!("{function}/description")),
            Some(&json!(
                "Invoke CortexFS tool shell. Pass exact tsh argv in args. The host returns a bounded UTF-8 observation with status ok or error; inspect errors before the next call."
            ))
        );
    }
    Ok(())
}
