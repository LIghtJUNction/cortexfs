use cortexfs_protocol::{
    Message, ModelRequest, ToolDefinition, WireProtocol, encode_model_request,
};
use serde_json::{Value, json};

#[test]
fn generic_tools_do_not_claim_openai_strict_schema() -> Result<(), Box<dyn std::error::Error>> {
    let schema = json!({"type": "object"});
    let mut request = ModelRequest::new("model", vec![Message::user("use the tool")]);
    request.tools.push(ToolDefinition {
        name: "lookup".to_owned(),
        description: Some("look up data".to_owned()),
        parameters: schema.clone(),
    });

    for protocol in [WireProtocol::OpenAiChat, WireProtocol::OpenAiResponses] {
        let encoded = encode_model_request(protocol, &request)?;
        let value: Value = serde_json::from_slice(&encoded)?;
        let tool = &value["tools"][0];
        let function = if protocol == WireProtocol::OpenAiChat {
            &tool["function"]
        } else {
            tool
        };

        assert_eq!(function.get("name").and_then(Value::as_str), Some("lookup"));
        assert_eq!(
            function.get("description").and_then(Value::as_str),
            Some("look up data")
        );
        assert_eq!(function.get("parameters"), Some(&schema));
        assert!(function.get("strict").is_none(), "{protocol:?}");
    }
    Ok(())
}
