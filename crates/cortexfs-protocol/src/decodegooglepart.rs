use crate::gemini::Content as NativeContent;
use crate::{Content, ContentPart, ConversionError, Message, Role, ToolCall};
use serde_json::{Value, json, value::RawValue};

fn raw(field: &str, value: &RawValue) -> Result<Value, ConversionError> {
    crate::semantic::raw_value(crate::WireProtocol::Gemini, field, value)
}

pub(super) fn message(source: &NativeContent<'_>) -> Result<Message, ConversionError> {
    let role = source.role.as_ref().map_or("user", |value| value.as_ref());
    let mut values = Vec::new();
    let mut calls = Vec::new();
    for (index, part) in source.parts.iter().enumerate() {
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
        if let Some(call) = part.function_call.as_ref() {
            let id = crate::gemini::correlation_id(call.id.as_deref(), index);
            if let Some(signature) = part.thought_signature.as_ref() {
                values.push(ContentPart::Data {
                    name: format!("gemini.thought_signature:{}", calls.len()),
                    value: json!(signature),
                });
            }
            calls.push(ToolCall {
                id,
                name: call.name.to_string(),
                arguments: raw("contents[].functionCall.args", call.args)?,
            });
        }
        if let Some(response) = part.function_response.as_ref() {
            values.push(ContentPart::Data {
                name: "gemini.function_response".to_owned(),
                value: raw("contents[].functionResponse.response", response.response)?,
            });
        }
        if part.thought == Some(true) {
            values.push(ContentPart::Data {
                name: "gemini.thought".to_owned(),
                value: Value::Bool(true),
            });
        }
    }
    Ok(Message {
        role: Role::new(if role == "model" { "assistant" } else { role }),
        content: Content::Parts(values),
        name: None,
        tool_call_id: None,
        tool_calls: calls,
    })
}

pub(super) fn tool(
    source: &crate::gemini::Function<'_>,
) -> Result<crate::ToolDefinition, ConversionError> {
    Ok(crate::ToolDefinition {
        name: source.name.to_string(),
        description: source.description.as_ref().map(ToString::to_string),
        parameters: source.parameters.map_or_else(
            || Ok(json!({})),
            |value| raw("tools[].functionDeclarations[].parameters", value),
        )?,
    })
}
