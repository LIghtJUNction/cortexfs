use crate::decodechoice::openai_unsupported;
use crate::openaichat::{Content as NativeContent, Request};
use crate::{Content, ContentPart, ConversionError, Message, ModelRequest, Role, ToolCall};
use serde_json::Value;

pub(super) fn request(input: &[u8]) -> Result<ModelRequest, ConversionError> {
    let source: Request<'_> = crate::semantic::parse(crate::WireProtocol::OpenAiChat, input)?;
    let messages = source
        .messages
        .iter()
        .map(message)
        .collect::<Result<Vec<_>, _>>()?;
    let mut result = ModelRequest::new(source.model.as_ref(), messages);
    result.stream = source.stream;
    result.max_output_tokens = source.max_tokens;
    result.tools = source.tools.iter().map(tool).collect::<Result<_, _>>()?;
    result.tool_choice = source
        .tool_choice
        .as_ref()
        .map(crate::decodechoice::openai)
        .transpose()?;
    for (name, raw) in &source.extra {
        result.options.insert(
            name.to_string(),
            crate::semantic::raw_value(crate::WireProtocol::OpenAiChat, name, raw)?,
        );
    }
    Ok(result)
}

fn message(source: &crate::openaichat::Message<'_>) -> Result<Message, ConversionError> {
    let content = source
        .content
        .as_ref()
        .map_or_else(|| Ok(Content::text("")), content)?;
    let tool_calls = source
        .tool_calls
        .iter()
        .map(|call| {
            let arguments = call.function.arguments.as_deref().map_or_else(
                || Ok(Value::Object(serde_json::Map::new())),
                |value| {
                    crate::semantic::json_value(
                        crate::WireProtocol::OpenAiChat,
                        "messages[].tool_calls[].function.arguments",
                        value,
                    )
                },
            )?;
            Ok(ToolCall {
                id: call.id.to_string(),
                name: call.function.name.to_string(),
                arguments,
            })
        })
        .collect::<Result<Vec<_>, ConversionError>>()?;
    Ok(Message {
        role: Role::new(source.role.as_ref()),
        content,
        name: source.name.as_ref().map(ToString::to_string),
        tool_call_id: source.tool_call_id.as_ref().map(ToString::to_string),
        tool_calls,
    })
}
fn content(source: &NativeContent<'_>) -> Result<Content, ConversionError> {
    match *source {
        NativeContent::Text(ref value) => Ok(Content::text(value.as_ref())),
        NativeContent::Parts(ref parts) => {
            let values = parts
                .iter()
                .map(|part| {
                    if part.kind.as_ref() == "text" {
                        return part.text.as_ref().map_or_else(
                            || Err(openai_unsupported("messages[].content[].text")),
                            |text| Ok(ContentPart::text(text.as_ref())),
                        );
                    }
                    if part.kind.as_ref() == "image_url" {
                        return part.image_url.as_ref().map_or_else(
                            || Err(openai_unsupported("messages[].content[].image_url")),
                            |image| {
                                Ok(ContentPart::Image {
                                    uri: image.url.to_string(),
                                    mime: None,
                                })
                            },
                        );
                    }
                    Err(openai_unsupported("messages[].content[].type"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Content::Parts(values))
        }
    }
}
fn tool(source: &crate::openaichat::Tool<'_>) -> Result<crate::ToolDefinition, ConversionError> {
    if source.kind.as_ref() != "function" {
        return Err(openai_unsupported("tools[].type"));
    }
    Ok(crate::ToolDefinition {
        name: source.function.name.to_string(),
        description: source
            .function
            .description
            .as_ref()
            .map(ToString::to_string),
        parameters: source.function.parameters.map_or_else(
            || Ok(Value::Object(serde_json::Map::new())),
            |raw| {
                crate::semantic::raw_value(
                    crate::WireProtocol::OpenAiChat,
                    "tools[].function.parameters",
                    raw,
                )
            },
        )?,
    })
}
