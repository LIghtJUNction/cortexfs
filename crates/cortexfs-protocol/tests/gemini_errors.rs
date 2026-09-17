use cortexfs_protocol::{EventStatus, ModelEvent, WireProtocol, decode_response_events};
use serde_json::json;
use std::error::Error;

#[test]
fn error_only_responses_normalize_without_model_metadata() -> Result<(), Box<dyn Error>> {
    for (body, code, message) in [
        (
            json!({"error": {"code": 400, "message": "API key not valid"}}),
            "provider_error",
            "API key not valid",
        ),
        (
            json!({"promptFeedback": {"blockReason": "SAFETY"}}),
            "SAFETY",
            "prompt blocked: SAFETY",
        ),
    ] {
        let bytes = serde_json::to_vec(&body)?;
        let events = decode_response_events(WireProtocol::Gemini, &bytes)?;
        assert!(matches!(
            events.first(),
            Some(ModelEvent::Start { model, .. }) if model == "unknown"
        ));
        assert!(events.iter().any(|event| matches!(
            event,
            ModelEvent::Error { error, .. }
                if error.code.as_str() == code
                    && error.message.as_str() == message
                    && !error.retryable
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ModelEvent::Done {
                status: EventStatus::Error,
                ..
            }
        )));
    }
    Ok(())
}
