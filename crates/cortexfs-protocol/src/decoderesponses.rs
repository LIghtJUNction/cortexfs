use crate::openairesponses::{Input, Item, Request};
use crate::{Content, ContextState, ConversionError, Message, ModelRequest, Role};

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
    result.stream = source.stream;
    result.max_output_tokens = source.max_output_tokens;
    result.tools = source
        .tools
        .iter()
        .map(crate::decoderesponsepart::tool)
        .collect::<Result<_, _>>()?;
    if let Some(reference) = source.previous_response_id.as_ref() {
        result.context = ContextState::provider_owned(
            "openai.responses.previous_response_id",
            reference.as_ref(),
        );
    } else if let Some(reference) = source.conversation.as_ref() {
        result.context =
            ContextState::provider_owned("openai.responses.conversation", reference.as_ref());
    }
    for (name, raw) in &source.extra {
        result.options.insert(
            name.to_string(),
            crate::semantic::raw_value(crate::WireProtocol::OpenAiResponses, name, raw)?,
        );
    }
    Ok(result)
}

fn item(source: &Item<'_>) -> Result<Message, ConversionError> {
    match *source {
        Item::Message {
            ref role,
            ref content,
        } => Ok(Message {
            role: Role::new(role.as_ref()),
            content: crate::decoderesponsepart::parts(content)?,
            name: None,
            tool_call_id: None,
            tool_calls: Vec::new(),
        }),
        Item::FunctionCall {
            ref call_id,
            ref name,
            ref arguments,
        } => Ok(Message {
            role: Role::new("assistant"),
            content: Content::text(""),
            name: None,
            tool_call_id: None,
            tool_calls: vec![crate::ToolCall {
                id: call_id.to_string(),
                name: name.to_string(),
                arguments: crate::semantic::json_value(
                    crate::WireProtocol::OpenAiResponses,
                    "input[].arguments",
                    arguments.as_ref(),
                )?,
            }],
        }),
        Item::FunctionCallOutput {
            ref call_id,
            ref output,
        } => Ok(Message {
            role: Role::new("tool"),
            content: Content::text(output.as_ref()),
            name: None,
            tool_call_id: Some(call_id.to_string()),
            tool_calls: Vec::new(),
        }),
    }
}
