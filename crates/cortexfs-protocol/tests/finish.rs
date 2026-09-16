use cortexfs_protocol::{EventStatus, ModelEvent, WireProtocol, decode_response_events};
use serde_json::json;

#[test]
fn openai_unusable_finish_reasons_are_errors() -> Result<(), Box<dyn std::error::Error>> {
    for reason in ["length", "content_filter"] {
        let input = serde_json::to_vec(&json!({
            "id": "run",
            "model": "model",
            "choices": [{
                "message": {"role": "assistant", "content": "partial"},
                "finish_reason": reason
            }]
        }))?;
        let events = decode_response_events(WireProtocol::OpenAiChat, &input)?;
        assert!(
            events.iter().any(|event| matches!(
                event,
                ModelEvent::Done {
                    status: EventStatus::Error,
                    ..
                }
            )),
            "{reason}: {events:?}"
        );
    }
    Ok(())
}
