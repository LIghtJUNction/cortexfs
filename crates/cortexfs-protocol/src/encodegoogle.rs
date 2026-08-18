use crate::{Content, ConversionError, Message, ModelRequest, WireProtocol};
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
    let config = generation(request);
    if !config.is_empty() {
        root.insert("generationConfig".to_owned(), Value::Object(config));
    }
    crate::encode::options(&mut root, request);
    let value = Value::Object(root);
    crate::encode::bytes(WireProtocol::Gemini, &value)
}

fn generation(request: &ModelRequest) -> Map<String, Value> {
    let mut value = Map::new();
    if let Some(tokens) = request.max_output_tokens {
        value.insert("maxOutputTokens".to_owned(), json!(tokens));
    }
    if let Some(config) = request.options.get("gemini.thinking_config") {
        value.insert("thinkingConfig".to_owned(), config.clone());
    }
    value
}

fn content(source: &Message) -> Result<Value, ConversionError> {
    let role = if source.role.as_str() == "assistant" {
        "model"
    } else {
        source.role.as_str()
    };
    let mut value = Map::new();
    value.insert("role".to_owned(), Value::String(role.to_owned()));
    let mut values = parts(&source.content, role)?;
    values.extend(
        source
            .tool_calls
            .iter()
            .map(|call| json!({"functionCall": {"name": call.name, "args": call.arguments}})),
    );
    if source.role.as_str() == "tool" {
        values.push(json!({"functionResponse": {"name": source.tool_call_id, "response": {"content": source.content.text_value()}}}));
    }
    value.insert("parts".to_owned(), Value::Array(values));
    Ok(Value::Object(value))
}

fn parts(content: &Content, role: &str) -> Result<Vec<Value>, ConversionError> {
    match *content {
        Content::Text(ref text) => Ok(vec![json!({"text": text})]),
        Content::Parts(ref parts) => parts.iter().map(|part| match *part {
            crate::ContentPart::Text { ref text } => Ok(json!({"text": text})),
            crate::ContentPart::Image { ref uri, ref mime } | crate::ContentPart::Audio { ref uri, ref mime } => Ok(json!({"fileData": {"mimeType": mime.as_deref().unwrap_or(if role == "model" { "application/octet-stream" } else { "image/*" }), "fileUri": uri}})),
            crate::ContentPart::Data { .. } => Err(ConversionError::UnsupportedField { protocol: WireProtocol::Gemini, field: "content part".to_owned() }),
        }).collect(),
    }
}
