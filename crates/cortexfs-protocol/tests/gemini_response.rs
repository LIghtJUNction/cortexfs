#[cfg(test)]
mod tests {
    use cortexfs_protocol::{ConversionError, ModelEvent, WireProtocol, decode_response_events};
    use serde_json::{Value, json};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn response(call: Value) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(&json!({
            "responseId": "run",
            "modelVersion": "gemini-model",
            "candidates": [{
                "content": {"role": "model", "parts": [{"functionCall": call}]},
                "finishReason": "STOP"
            }]
        }))
    }

    #[test]
    fn gemini_rejects_empty_candidates() {
        let input = br#"{"modelVersion":"gemini-model","candidates":[]}"#;
        assert!(decode_response_events(WireProtocol::Gemini, input).is_err());
    }

    #[test]
    fn malformed_function_calls_fail_at_the_gemini_boundary() -> TestResult {
        for (call, field) in [
            (json!({"args": {}}), "functionCall.name"),
            (json!({"name": 7}), "functionCall.name"),
            (json!({"name": ""}), "functionCall.name"),
            (json!({"name": "lookup", "args": []}), "functionCall.args"),
            (json!({"name": "lookup", "id": 7}), "functionCall.id"),
        ] {
            let input = response(call)?;
            assert_eq!(
                decode_response_events(WireProtocol::Gemini, &input),
                Err(ConversionError::InvalidField {
                    protocol: WireProtocol::Gemini,
                    field: field.to_owned(),
                })
            );
        }
        Ok(())
    }

    #[test]
    fn optional_function_call_fields_keep_the_neutral_shape() -> TestResult {
        let input = response(json!({"name": "lookup"}))?;
        let events = decode_response_events(WireProtocol::Gemini, &input)?;
        assert!(events.iter().any(|event| matches!(
            event,
            ModelEvent::ToolCall { call, .. }
                if call.id == "lookup" && call.name == "lookup" && call.arguments == json!({})
        )));
        Ok(())
    }
}
