use crate::object::runner::{
    ProviderCredential, ResolvedTransport, parse_provider_content, parse_provider_usage,
    provider_request_body, provider_request_target,
};
use cortexfs_protocol::EventStatus::Error;
use cortexfs_protocol::{ModelEvent, WireProtocol, decode_response_events};
use serde_json::{Value, json};
const BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
fn direct(base_url: &str) -> ResolvedTransport {
    ResolvedTransport::Direct {
        base_url: base_url.to_owned(),
    }
}

fn target(
    transport: &ResolvedTransport,
    credential: &ProviderCredential,
    model: &str,
) -> Result<(String, Vec<String>), String> {
    provider_request_target(
        transport,
        Some(credential),
        WireProtocol::Gemini,
        model,
        "run",
    )
    .map(|(target, headers)| (target.url, headers))
}

#[test]
fn generate_content_target_keeps_the_provider_api_version() -> Result<(), String> {
    let key = ProviderCredential::GoogleApiKey("secret".to_owned());
    let expected = format!("{BASE_URL}/models/gemini-2.5-flash:generateContent");
    for (base_url, model) in [
        (BASE_URL.to_owned(), "gemini-2.5-flash"),
        (format!("{BASE_URL}/"), "gemini-2.5-flash"),
        (BASE_URL.to_owned(), "models/gemini-2.5-flash"),
    ] {
        assert_eq!(
            target(&direct(&base_url), &key, model)?,
            (expected.clone(), vec!["x-goog-api-key: secret".to_owned()])
        );
    }
    Ok(())
}

#[test]
fn generate_content_target_rejects_path_traversal_and_wrong_credentials() {
    let key = ProviderCredential::GoogleApiKey("secret".to_owned());
    let invalid = target(&direct(BASE_URL), &key, "../models/other");
    assert_eq!(invalid, Err("invalid Gemini model name".to_owned()));
    for credential in [
        ProviderCredential::AnthropicApiKey("secret".to_owned()),
        ProviderCredential::Codex {
            token: "secret".to_owned(),
            account_id: "account".to_owned(),
        },
    ] {
        let invalid = target(&direct(BASE_URL), &credential, "gemini-2.5-flash");
        assert_eq!(invalid, Err("invalid Gemini credential".to_owned()));
    }
    let missing =
        provider_request_target(&direct(BASE_URL), None, WireProtocol::Gemini, "m", "run");
    assert_eq!(missing.err().as_deref(), Some("missing Gemini credential"));
}

#[test]
fn oauth_access_tokens_use_the_bearer_header() -> Result<(), String> {
    let token = ProviderCredential::Bearer("token".to_owned());
    let (_url, headers) = target(&direct(BASE_URL), &token, "gemini-2.5-flash")?;
    assert_eq!(headers, vec!["Authorization: Bearer token".to_owned()]);
    Ok(())
}

#[test]
fn request_body_drops_path_bound_and_openai_only_fields() -> Result<(), Box<dyn std::error::Error>>
{
    let body = provider_request_body(
        WireProtocol::Gemini,
        "gemini-2.5-flash",
        "hello",
        false,
        cortexfs::ModelEffort::Auto,
        true,
    )?;
    let value = serde_json::from_str::<Value>(&body)?;
    let absent = ["model", "parallel_tool_calls", "stream"];
    assert!(absent.iter().all(|key| value.get(*key).is_none()));
    let name = value.pointer("/tools/0/functionDeclarations/0/name");
    let text = value.pointer("/contents/0/parts/0/text");
    assert_eq!((name, text), (Some(&json!("tsh")), Some(&json!("hello"))));
    Ok(())
}

#[test]
fn responses_decode_text_tool_calls_and_usage() -> Result<(), Box<dyn std::error::Error>> {
    let protocol = WireProtocol::Gemini;
    let text = candidate(&json!([{"text": "hello"}]), "STOP");
    assert_eq!(parse_provider_content(protocol, text.as_bytes())?, "hello");
    let explicit = json!({"functionCall":{"id":"call-1","name":"tsh","args":{"args":[]}}});
    let legacy = json!({"functionCall":{"name":"tsh","args":{"args":[]}}});
    for (part, id) in [(explicit, "call-1"), (legacy, "gemini-call-0")] {
        let body = candidate(&json!([part]), "STOP");
        let call = parse_provider_content(protocol, body.as_bytes())?;
        let value = serde_json::from_str::<Value>(&call)?;
        assert_eq!(value.get("id"), Some(&json!(id)));
        assert_eq!(value.get("name"), Some(&json!("tsh")));
        assert_eq!(value.get("arguments"), Some(&json!({"args": []})));
    }
    let usage_json = json!({"usageMetadata": {"promptTokenCount": 11, "candidatesTokenCount": 7, "cachedContentTokenCount": 3}}).to_string();
    let usage = parse_provider_usage(usage_json.as_bytes())?.ok_or("gemini usage metadata")?;
    let counts = (usage.input_tokens, usage.output_tokens, usage.cached_tokens);
    assert_eq!(counts, (11, 7, Some(3)));
    Ok(())
}

#[test]
fn responses_surface_provider_errors_and_refused_candidates() {
    let protocol = WireProtocol::Gemini;
    for (response, expected) in [
        (
            json!({"error": {"code": 400, "message": "API key not valid"}}).to_string(),
            "provider response failed: API key not valid",
        ),
        (
            json!({"promptFeedback": {"blockReason": "SAFETY"}}).to_string(),
            "provider response blocked with SAFETY",
        ),
        (
            candidate(&json!([{"text": "part"}]), "MAX_TOKENS"),
            "provider response finished with MAX_TOKENS",
        ),
    ] {
        let out = decode_response_events(protocol, response.as_bytes()).unwrap_or_default();
        assert!(
            matches!(out.get(1), Some(ModelEvent::Error { .. })) || expected.contains("finished")
        );
        let done = out.last();
        assert!(matches!(done, Some(ModelEvent::Done { status: Error, .. })));
        let actual = parse_provider_content(protocol, response.as_bytes()).err();
        assert_eq!(actual, Some(expected.to_owned()));
    }
    for response in [
        json!({"candidates": [{"finishReason": "STOP"}]}),
        json!({"modelVersion": "gemini-2.5-flash", "candidates": [{}]}),
        json!({"modelVersion": "gemini-2.5-flash", "candidates": [{"finishReason": null}]}),
    ] {
        assert!(decode_response_events(protocol, response.to_string().as_bytes()).is_err());
    }
}

fn candidate(parts: &Value, finish_reason: &str) -> String {
    json!({
        "responseId": "response-a",
        "modelVersion": "gemini-2.5-flash",
        "candidates": [{"content": {"role": "model", "parts": parts}, "finishReason": finish_reason}],
    })
    .to_string()
}
