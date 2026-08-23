use crate::object::runner::{
    ProviderCredential, ResolvedTransport, parse_provider_content, parse_provider_usage,
    provider_request_body, provider_request_target,
};
use cortexfs_protocol::WireProtocol;
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
    assert_eq!(
        target(&direct(BASE_URL), &key, "../models/other"),
        Err("invalid Gemini model name".to_owned())
    );
    for credential in [
        ProviderCredential::AnthropicApiKey("secret".to_owned()),
        ProviderCredential::Codex {
            token: "secret".to_owned(),
            account_id: "account".to_owned(),
        },
    ] {
        assert_eq!(
            target(&direct(BASE_URL), &credential, "gemini-2.5-flash"),
            Err("invalid Gemini credential".to_owned())
        );
    }
    assert_eq!(
        provider_request_target(&direct(BASE_URL), None, WireProtocol::Gemini, "m", "run")
            .err()
            .as_deref(),
        Some("missing Gemini credential")
    );
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
    assert_eq!(value.get("model"), None);
    assert_eq!(value.get("parallel_tool_calls"), None);
    assert_eq!(value.get("stream"), None);
    assert_eq!(
        value.pointer("/tools/0/functionDeclarations/0/name"),
        Some(&json!("tsh"))
    );
    assert_eq!(
        value.pointer("/contents/0/parts/0/text"),
        Some(&json!("hello"))
    );
    Ok(())
}

#[test]
fn responses_decode_text_tool_calls_and_usage() -> Result<(), Box<dyn std::error::Error>> {
    let text = candidate(&json!([{"text": "hello"}]), "STOP");
    assert_eq!(
        parse_provider_content(WireProtocol::Gemini, text.as_bytes())?,
        "hello"
    );
    let call = candidate(
        &json!([{"functionCall": {"name": "tsh", "args": {"args": ["ls"]}}}]),
        "STOP",
    );
    let call = parse_provider_content(WireProtocol::Gemini, call.as_bytes())?;
    assert_eq!(
        serde_json::from_str::<Value>(&call)?,
        json!({"type": "tool_call", "id": "tsh", "name": "tsh", "arguments": {"args": ["ls"]}})
    );
    let usage = parse_provider_usage(
        json!({"usageMetadata": {"promptTokenCount": 11, "candidatesTokenCount": 7, "cachedContentTokenCount": 3}})
            .to_string()
            .as_bytes(),
    )?
    .ok_or("gemini usage metadata")?;
    assert_eq!(
        (usage.input_tokens, usage.output_tokens, usage.cached_tokens),
        (11, 7, Some(3))
    );
    Ok(())
}

#[test]
fn responses_surface_provider_errors_and_refused_candidates() {
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
        assert_eq!(
            parse_provider_content(WireProtocol::Gemini, response.as_bytes()).err(),
            Some(expected.to_owned())
        );
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
