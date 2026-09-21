use crate::{Content, ContentPart, ConversionError, Message, ModelRequest, WireProtocol};
use serde_json::{Map, Value, json};

pub(super) fn request(request: &ModelRequest) -> Result<Vec<u8>, ConversionError> {
    crate::encode::check_context(request, WireProtocol::Gemini)?;
    let mut root = Map::new();
    root.insert("model".to_owned(), Value::String(request.model.clone()));
    if let Some(system) = request
        .messages
        .iter()
        .find(|message| message.role.as_str() == "system")
    {
        root.insert(
            "systemInstruction".to_owned(),
            json!({"parts": parts(&system.content, "system")?}),
        );
    }
    root.insert(
        "contents".to_owned(),
        Value::Array(
            request
                .messages
                .iter()
                .filter(|message| message.role.as_str() != "system")
                .map(content)
                .collect::<Result<_, _>>()?,
        ),
    );
    if !request.tools.is_empty() {
        root.insert("tools".to_owned(), json!([{"functionDeclarations": request.tools.iter().map(|tool| json!({"name": tool.name, "description": tool.description, "parameters": tool.parameters})).collect::<Vec<_>>() }]));
    }
    let mut config = Map::new();
    if let Some(tokens) = request.max_output_tokens {
        config.insert("maxOutputTokens".to_owned(), json!(tokens));
    }
    if let Some(thinking) = request.options.get("gemini.thinking_config") {
        config.insert("thinkingConfig".to_owned(), thinking.clone());
    }
    if !config.is_empty() {
        root.insert("generationConfig".to_owned(), Value::Object(config));
    }
    crate::encode::options(&mut root, request);
    crate::encode::bytes(WireProtocol::Gemini, &Value::Object(root))
}

fn content(source: &Message) -> Result<Value, ConversionError> {
    if source.role.as_str() == "tool" {
        let name = source.name.as_ref().or(source.tool_call_id.as_ref());
        return Ok(
            json!({"role": "user", "parts": [{"functionResponse": {"id": source.tool_call_id, "name": name, "response": {"content": source.content.text_value()}}}]}),
        );
    }
    let role = match source.role.as_str() {
        "assistant" => "model",
        role => role,
    };
    let mut values = parts(&source.content, role)?;
    values.extend(source.tool_calls.iter().map(|call| {
        let mut value =
            json!({"functionCall": {"id": call.id, "name": call.name, "args": call.arguments}});
        let signature = match &source.content {
            Content::Parts(parts) => parts.iter().find_map(|part| match part {
                ContentPart::Data { name, value } => (name
                    .strip_prefix("gemini.thought_signature:")
                    == Some(call.id.as_str()))
                .then_some(value)
                .and_then(Value::as_str),
                _ => None,
            }),
            Content::Text(_) => None,
        };
        if let Some(signature) = signature {
            value["thoughtSignature"] = Value::String(signature.to_owned());
        }
        value
    }));
    Ok(json!({"role": role, "parts": values}))
}

fn parts(content: &Content, role: &str) -> Result<Vec<Value>, ConversionError> {
    match *content {
        Content::Text(ref text) => Ok(vec![json!({"text": text})]),
        Content::Parts(ref parts) => parts
            .iter()
            .filter_map(|part| match *part {
                ContentPart::Text { ref text } => Some(Ok(json!({"text": text}))),
                ContentPart::Image { ref uri, ref mime } | ContentPart::Audio { ref uri, ref mime } => Some(Ok(json!({"fileData": {"mimeType": mime.as_deref().unwrap_or(if role == "model" { "application/octet-stream" } else { "image/*" }), "fileUri": uri}}))),
                ContentPart::Data { ref name, .. }
                    if name.starts_with("gemini.thought_signature:") => None,
                ContentPart::Data { .. } => Some(Err(ConversionError::UnsupportedField {
                    protocol: WireProtocol::Gemini,
                    field: "content part".to_owned(),
                })),
            })
            .collect(),
    }
}
