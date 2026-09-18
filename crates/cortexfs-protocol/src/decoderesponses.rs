use crate::openairesponses::{Input, Item, Request};
use crate::{ContextState, ConversionError, Message, ModelRequest, ToolCall};
pub(super) fn request(input: &[u8]) -> Result<ModelRequest, ConversionError> {
    let source: Request<'_> = crate::semantic::parse(crate::WireProtocol::OpenAiResponses, input)?;
    let mut messages = Vec::new();
    if let Some(instructions) = source.instructions.as_ref() {
        messages.push(Message::system(instructions.as_ref()));
    }
    if let Some(input) = source.input.as_ref() {
        match *input {
            Input::Text(ref text) => messages.push(Message::user(text.as_ref())),
            Input::Items(ref items) => {
                messages.extend(items.iter().map(item).collect::<Result<Vec<_>, _>>()?);
            }
        }
    }
    if messages.is_empty() {
        return Err(ConversionError::MissingField {
            protocol: crate::WireProtocol::OpenAiResponses,
            field: "input".to_owned(),
        });
    }
    let mut result = ModelRequest::new(source.model.as_ref(), messages);
    (result.stream, result.max_output_tokens) = (source.stream, source.max_output_tokens);
    for tool in &source.tools {
        result.tools.push(crate::decoderesponsepart::tool(tool)?);
    }
    if let Some(id) = source.previous_response_id.as_ref() {
        let kind = "openai.responses.previous_response_id";
        result.context = ContextState::provider_owned(kind, id.as_ref());
    } else if let Some(id) = source.conversation.as_ref() {
        result.context = ContextState::provider_owned("openai.responses.conversation", id.as_ref());
    }
    for (name, raw) in &source.extra {
        let value = crate::semantic::raw_value(crate::WireProtocol::OpenAiResponses, name, raw)?;
        result.options.insert(name.to_string(), value);
    }
    Ok(result)
}
fn item(source: &Item<'_>) -> Result<Message, ConversionError> {
    match *source {
        Item::Message {
            ref role,
            ref content,
        } => {
            let mut message = Message::new(role.as_ref(), "");
            message.content = crate::decoderesponsepart::parts(content)?;
            Ok(message)
        }
        Item::FunctionCall {
            ref call_id,
            ref name,
            ref arguments,
        } => {
            let mut message = Message::assistant("");
            message.tool_calls.push(ToolCall {
                id: identifier(call_id, "input[].call_id")?,
                name: identifier(name, "input[].name")?,
                arguments: crate::semantic::json_value(
                    crate::WireProtocol::OpenAiResponses,
                    "input[].arguments",
                    arguments.as_ref(),
                )?,
            });
            Ok(message)
        }
        Item::FunctionCallOutput {
            ref call_id,
            ref output,
        } => {
            let mut message = Message::new("tool", output.as_ref());
            message.tool_call_id = Some(identifier(call_id, "input[].call_id")?);
            Ok(message)
        }
    }
}
fn identifier(value: &str, field: &str) -> Result<String, ConversionError> {
    (!value.trim().is_empty())
        .then(|| value.to_owned())
        .ok_or_else(|| ConversionError::InvalidField {
            protocol: crate::WireProtocol::OpenAiResponses,
            field: field.to_owned(),
        })
}
