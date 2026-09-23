use cortexfs_protocol::{
    ContextOwnership, WireProtocol, decode_model_request, encode_model_request,
};
use serde_json::Value;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn responses_conversation_forms_share_one_context_reference() -> TestResult {
    for (input, id) in [
        (
            br#"{"model":"responses-model","conversation":"conv_42","input":"next"}"#.as_slice(),
            "conv_42",
        ),
        (
            br#"{"model":"responses-model","conversation":{"id":"conv_43"},"input":"next"}"#.as_slice(),
            "conv_43",
        ),
    ] {
        let request = decode_model_request(WireProtocol::OpenAiResponses, input)?;
        assert_eq!(request.context.ownership, ContextOwnership::ProviderOwned);
        assert_eq!(
            request.context.reference.as_ref().map(|reference| (
                reference.namespace.as_str(),
                reference.value.as_str(),
            )),
            Some(("openai.responses.conversation", id)),
        );
        let encoded = encode_model_request(WireProtocol::OpenAiResponses, &request)?;
        let value: Value = serde_json::from_slice(&encoded)?;
        assert_eq!(value.get("conversation").and_then(Value::as_str), Some(id));
    }
    Ok(())
}
