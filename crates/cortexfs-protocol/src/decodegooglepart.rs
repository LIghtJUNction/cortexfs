use crate::gemini::Content as NativeContent;
use crate::{Content, ContentPart, ConversionError, Message, Role, ToolCall};
use serde_json::{Value, json};

pub(super) fn message(source: &NativeContent<'_>) -> Result<Message, ConversionError> {
    let role = source.role.as_ref().map_or("user", |value| value.as_ref());
    let mut calls = Vec::new();
    for part in &source.parts {
        if let Some(call) = part.function_call.as_ref() {
            calls.push(ToolCall {
                id: call.name.to_string(),
                name: call.name.to_string(),
                arguments: crate::semantic::raw_value(
                    crate::WireProtocol::Gemini,
                    "contents[].functionCall.args",
                    call.args,
                )?,
            });
        }
    }
    Ok(Message {
        role: Role::new(if role == "model" { "assistant" } else { role }),
        content: content(source)?,
        name: None,
        tool_call_id: None,
        tool_calls: calls,
    })
}

pub(super) fn content(source: &NativeContent<'_>) -> Result<Content, ConversionError> {
    let mut values = Vec::new();
    for part in &source.parts {
        if let Some(text) = part.text.as_ref() {
            values.push(ContentPart::text(text.as_ref()));
        }
        if let Some(file) = part.file_data.as_ref() {
            values.push(ContentPart::Image {
                uri: file.file_uri.to_string(),
                mime: Some(file.mime_type.to_string()),
            });
        }
        if let Some(blob) = part.inline_data.as_ref() {
            values.push(ContentPart::Data {
                name: "gemini.inline_data".to_owned(),
                value: json!({"mime_type": blob.mime_type, "data": blob.data}),
            });
        }
        if let Some(response) = part.function_response.as_ref() {
            values.push(ContentPart::Data {
                name: "gemini.function_response".to_owned(),
                value: crate::semantic::raw_value(
                    crate::WireProtocol::Gemini,
                    "contents[].functionResponse.response",
                    response.response,
                )?,
            });
        }
        if part.thought == Some(true) {
            values.push(ContentPart::Data {
                name: "gemini.thought".to_owned(),
                value: Value::Bool(true),
            });
        }
    }
    Ok(Content::Parts(values))
}

pub(super) fn tool(
    source: &crate::gemini::Function<'_>,
) -> Result<crate::ToolDefinition, ConversionError> {
    Ok(crate::ToolDefinition {
        name: source.name.to_string(),
        description: source.description.as_ref().map(ToString::to_string),
        parameters: source.parameters.map_or_else(
            || Ok(Value::Object(serde_json::Map::new())),
            |raw| {
                crate::semantic::raw_value(
                    crate::WireProtocol::Gemini,
                    "tools[].functionDeclarations[].parameters",
                    raw,
                )
            },
        )?,
    })
}
