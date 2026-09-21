use crate::{Content, ContentPart, ConversionError, Message, ModelRequest, Role, ToolDefinition};
use serde_json::{Value, json};

pub(crate) fn request(source: &ModelRequest) -> Result<Vec<u8>, ConversionError> {
    let mut contents = Vec::new();
    for message in &source.messages {
        contents.push(content(message)?);
    }
    let system_instruction = source
        .system
        .as_ref()
        .map(|system| Message {
            role: Role::new("user"),
            content: Content::Text(system.clone()),
            name: None,
            tool_call_id: None,
            tool_calls: Vec::new(),
        })
        .map(|message| content(&message))
        .transpose()?;
    let mut request = json!({"contents": contents});
    if let Some(model) = source.model.as_ref() {
        request["model"] = json!(model);
    }
    if let Some(system) = system_instruction {
        request["systemInstruction"] = system;
    }
    if !source.tools.is_empty() {
        request["tools"] = json!([{"functionDeclarations": source.tools.iter().map(tool).collect::<Vec<_>>() }]);
    }
    if source.max_tokens.is_some() || source.thinking.is_some() {
        request["generationConfig"] = json!({
            "maxOutputTokens": source.max_tokens,
            "thinkingConfig": source.thinking,
        });
    }
    serde_json::to_vec(&request).map_err(ConversionError::serialize)
}

fn content(source: &Message) -> Result<Value, ConversionError> {
    let role = match source.role.as_str() {
        "assistant" => "model",
        role => role,
    };
    let mut values = parts(&source.content)?;
    values.extend(source.tool_calls.iter().enumerate().map(|(index, call)| {
        let mut value =
            json!({"functionCall": {"id": call.id, "name": call.name, "args": call.arguments}});
        let marker = format!("gemini.thought_signature:{index}");
        let signature = match source.content {
            Content::Parts(ref parts) => parts.iter().find_map(|part| match *part {
                ContentPart::Data { ref name, ref value } if name == &marker => value.as_str(),
                _ => None,
            }),
            Content::Text(_) => None,
        };
        if let (Some(signature), Some(object)) = (signature, value.as_object_mut()) {
            drop(object.insert("thoughtSignature".to_owned(), json!(signature)));
        }
        value
    }));
    Ok(json!({"role": role, "parts": values}))
}

fn parts(content: &Content) -> Result<Vec<Value>, ConversionError> {
    match content {
        Content::Text(text) => Ok(vec![json!({"text": text})]),
        Content::Parts(parts) => parts
            .iter()
            .filter(|part| {
                !matches!(part, ContentPart::Data { name, .. } if name.starts_with("gemini.thought_signature:"))
            })
            .map(|part| match part {
                ContentPart::Text { text } => Ok(json!({"text": text})),
                ContentPart::Image { uri, mime } => Ok(json!({"fileData": {
                    "fileUri": uri,
                    "mimeType": mime.as_deref().unwrap_or("application/octet-stream")
                }})),
                ContentPart::Data { .. } => Err(ConversionError::unsupported(
                    "contents[].parts[]",
                    "Gemini request data part",
                )),
            })
            .collect(),
    }
}

fn tool(source: &ToolDefinition) -> Value {
    json!({
        "name": source.name,
        "description": source.description,
        "parameters": source.parameters,
    })
}
