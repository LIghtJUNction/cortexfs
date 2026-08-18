#[cfg(test)]
mod tests {
    use cortexfs_protocol::{
        BridgePath, Content, ContextOwnership, Message, ModelEvent, ModelRequest, NativeRequest,
        ProtocolError, ToolDefinition, WireProtocol, decode_model_request, decode_native_request,
        decode_response_events, encode_model_request, encode_response_events, transcode_request,
        transcode_response,
    };
    use serde_json::json;
    use std::borrow::Cow;

    const CHAT: &[u8] = br#"{"model":"chat-model","messages":[{"role":"user","content":"hi"}]}"#;
    const RESPONSES: &[u8] = br#"{"model":"responses-model","input":"hi"}"#;
    const GEMINI: &[u8] =
        br#"{"model":"gemini-model","contents":[{"role":"user","parts":[{"text":"hi"}]}]}"#;
    const ANTHROPIC: &[u8] =
        br#"{"model":"claude-model","max_tokens":32,"messages":[{"role":"user","content":"hi"}]}"#;
    const CHAT_RESPONSE: &[u8] = br#"{"id":"chat-run","model":"chat-model","choices":[{"message":{"role":"assistant","content":"hello"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":2}}"#;
    const RESPONSES_RESPONSE: &[u8] = br#"{"id":"responses-run","model":"responses-model","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hello"}]}],"usage":{"input_tokens":3,"output_tokens":2}}"#;
    const GEMINI_RESPONSE: &[u8] = br#"{"responseId":"gemini-run","modelVersion":"gemini-model","candidates":[{"content":{"role":"model","parts":[{"text":"hello"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":2}}"#;
    const ANTHROPIC_RESPONSE: &[u8] = br#"{"id":"anthropic-run","model":"claude-model","role":"assistant","content":[{"type":"text","text":"hello"}],"stop_reason":"end_turn","usage":{"input_tokens":3,"output_tokens":2}}"#;
    const CHAT_TOOL_RESPONSE: &[u8] = br#"{"id":"chat-tool-run","model":"chat-model","choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call-1","type":"function","function":{"name":"lookup","arguments":"{\"q\":\"rust\"}"}}]},"finish_reason":"tool_calls"}]}"#;

    fn cases() -> [(WireProtocol, &'static [u8]); 4] {
        [
            (WireProtocol::OpenAiChat, CHAT),
            (WireProtocol::OpenAiResponses, RESPONSES),
            (WireProtocol::Gemini, GEMINI),
            (WireProtocol::Anthropic, ANTHROPIC),
        ]
    }

    #[test]
    fn request_is_provider_neutral_and_validates_tools() {
        let mut request = ModelRequest::new(
            "example/model",
            vec![
                Message::system("follow the contract"),
                Message::user("hello"),
            ],
        );
        request.tools.push(ToolDefinition {
            name: "tsh".to_owned(),
            description: Some("run a bounded tool".to_owned()),
            parameters: json!({"type":"object"}),
        });
        assert!(request.validate().is_ok());
        assert_eq!(
            request.messages.get(1).map(|message| &message.content),
            Some(&Content::text("hello"))
        );
    }

    #[test]
    fn events_serialize_with_stable_type_tags() {
        let event = ModelEvent::TextDelta {
            run: "run-1".to_owned(),
            text: "hi".to_owned(),
        };
        let value = serde_json::to_value(event).unwrap_or_default();
        assert_eq!(
            value.get("type").and_then(|item| item.as_str()),
            Some("text_delta")
        );
        assert_eq!(
            value.get("run").and_then(|item| item.as_str()),
            Some("run-1")
        );
    }

    #[test]
    fn native_ir_borrows_unescaped_wire_strings() {
        let decoded = decode_native_request(WireProtocol::OpenAiChat, CHAT);
        assert!(decoded.is_ok());
        if let Ok(NativeRequest::OpenAiChat(request)) = decoded {
            assert!(matches!(&request.model, Cow::Borrowed(_)));
            assert!(matches!(
                request
                    .messages
                    .first()
                    .and_then(|message| message.content.as_ref()),
                Some(cortexfs_protocol::openaichat::Content::Text(Cow::Borrowed(
                    _
                )))
            ));
        }
    }

    #[test]
    fn every_dialect_decodes_and_encodes_the_semantic_ir() {
        for (protocol, input) in cases() {
            let decoded = decode_model_request(protocol, input);
            assert!(decoded.is_ok(), "{protocol}: {decoded:?}");
            if let Ok(request) = decoded {
                assert!(request.validate().is_ok());
                let encoded = encode_model_request(protocol, &request);
                assert!(encoded.is_ok(), "{protocol}: {encoded:?}");
                if let Ok(bytes) = encoded {
                    assert!(serde_json::from_slice::<serde_json::Value>(&bytes).is_ok());
                }
            }
        }
    }

    #[test]
    fn conversion_matrix_covers_four_request_dialects() {
        for (source, input) in cases() {
            for (target, _) in cases() {
                let converted = transcode_request(source, target, input);
                assert!(converted.is_ok(), "{source}->{target}: {converted:?}");
                if let Ok(converted) = converted {
                    assert!(serde_json::from_slice::<serde_json::Value>(&converted.bytes).is_ok());
                    let reparsed = decode_model_request(target, &converted.bytes);
                    assert!(reparsed.is_ok(), "{source}->{target}: {reparsed:?}");
                    let expected = if source == target {
                        BridgePath::Identity
                    } else if matches!(
                        (source, target),
                        (WireProtocol::OpenAiChat, WireProtocol::Gemini)
                            | (WireProtocol::Gemini, WireProtocol::OpenAiChat)
                    ) {
                        BridgePath::Direct
                    } else {
                        BridgePath::ViaIr
                    };
                    assert_eq!(converted.path, expected);
                }
            }
        }
    }

    #[test]
    fn identity_route_preserves_bytes_exactly() {
        let result = transcode_request(WireProtocol::OpenAiChat, WireProtocol::OpenAiChat, CHAT);
        assert!(result.is_ok());
        if let Ok(result) = result {
            assert_eq!(result.path, BridgePath::Identity);
            assert_eq!(result.bytes, CHAT);
        }
    }

    #[test]
    fn responses_context_reference_is_semantic_metadata() {
        let input =
            br#"{"model":"responses-model","previous_response_id":"resp_42","input":"next"}"#;
        let decoded = decode_model_request(WireProtocol::OpenAiResponses, input);
        assert!(decoded.is_ok());
        if let Ok(request) = decoded {
            assert_eq!(request.context.ownership, ContextOwnership::ProviderOwned);
            assert_eq!(
                request
                    .context
                    .reference
                    .as_ref()
                    .map(|item| item.value.as_str()),
                Some("resp_42")
            );
            let encoded = encode_model_request(WireProtocol::OpenAiResponses, &request);
            assert!(encoded.is_ok());
            if let Ok(bytes) = encoded {
                let value = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_default();
                assert_eq!(
                    value
                        .get("previous_response_id")
                        .and_then(|item| item.as_str()),
                    Some("resp_42")
                );
            }
            let portable = encode_model_request(WireProtocol::OpenAiChat, &request);
            assert!(portable.is_err());
        }
    }

    #[test]
    fn invalid_provider_context_is_rejected_before_encoding() {
        let mut request = ModelRequest::new("model", vec![Message::user("hi")]);
        request.context.ownership = ContextOwnership::ProviderOwned;
        assert!(matches!(
            request.validate(),
            Err(ProtocolError::InvalidContext(_))
        ));
    }

    #[test]
    fn response_events_roundtrip_through_all_native_dialects() {
        let cases = [
            (WireProtocol::OpenAiChat, CHAT_RESPONSE),
            (WireProtocol::OpenAiResponses, RESPONSES_RESPONSE),
            (WireProtocol::Gemini, GEMINI_RESPONSE),
            (WireProtocol::Anthropic, ANTHROPIC_RESPONSE),
        ];
        for (protocol, input) in cases {
            let events = decode_response_events(protocol, input);
            assert!(events.is_ok(), "{protocol}: {events:?}");
            if let Ok(events) = events {
                assert!(
                    events
                        .iter()
                        .any(|event| matches!(event, ModelEvent::TextDelta { .. }))
                );
                let encoded = encode_response_events(protocol, &events);
                assert!(encoded.is_ok(), "{protocol}: {encoded:?}");
                if let Ok(encoded) = encoded {
                    assert!(serde_json::from_slice::<serde_json::Value>(&encoded).is_ok());
                }
            }
        }
    }

    #[test]
    fn response_conversion_matrix_supports_all_directions() {
        let cases = [
            (WireProtocol::OpenAiChat, CHAT_RESPONSE),
            (WireProtocol::OpenAiResponses, RESPONSES_RESPONSE),
            (WireProtocol::Gemini, GEMINI_RESPONSE),
            (WireProtocol::Anthropic, ANTHROPIC_RESPONSE),
        ];
        for (source, input) in cases {
            for (target, _) in cases {
                let converted = transcode_response(source, target, input);
                assert!(converted.is_ok(), "{source}->{target}: {converted:?}");
                if let Ok(converted) = converted {
                    assert!(serde_json::from_slice::<serde_json::Value>(&converted.bytes).is_ok());
                }
            }
        }
    }

    #[test]
    fn direct_route_keeps_image_and_tool_schema() {
        let input = br#"{"model":"gemini-model","messages":[{"role":"user","content":[{"type":"text","text":"find"},{"type":"image_url","image_url":{"url":"https://example.invalid/a.png"}}]}],"tools":[{"type":"function","function":{"name":"lookup","description":"lookup data","parameters":{"type":"object"}}}]}"#;
        let converted = transcode_request(WireProtocol::OpenAiChat, WireProtocol::Gemini, input);
        assert!(converted.is_ok());
        if let Ok(converted) = converted {
            assert_eq!(converted.path, BridgePath::Direct);
            let value =
                serde_json::from_slice::<serde_json::Value>(&converted.bytes).unwrap_or_default();
            assert!(value.get("tools").is_some());
            assert!(value.get("contents").is_some());
        }
    }

    #[test]
    fn response_tool_call_becomes_a_normalized_event() {
        let events = decode_response_events(WireProtocol::OpenAiChat, CHAT_TOOL_RESPONSE);
        assert!(events.is_ok());
        if let Ok(events) = events {
            assert!(
                events
                    .iter()
                    .any(|event| matches!(event, ModelEvent::ToolCall { .. }))
            );
            let encoded = encode_response_events(WireProtocol::Anthropic, &events);
            assert!(encoded.is_ok());
        }
    }

    #[test]
    fn malformed_native_json_returns_protocol_error() {
        let result = decode_model_request(WireProtocol::OpenAiChat, b"not-json");
        assert!(matches!(
            result,
            Err(cortexfs_protocol::ConversionError::InvalidJson { .. })
        ));
    }
}
