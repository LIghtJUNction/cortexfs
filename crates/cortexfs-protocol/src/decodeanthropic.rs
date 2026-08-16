use crate::anthropic::{Block, Content as NativeContent, Request};
use crate::{Content, ContextState, ConversionError, Message, ModelRequest, Role, ToolCall};

pub(super) fn request(input: &[u8]) -> Result<ModelRequest, ConversionError> {
    let source: Request<'_> = crate::semantic::parse(crate::WireProtocol::Anthropic, input)?;
    let mut messages = Vec::new();
    if let Some(system) = source.system.as_ref() {
        messages.push(Message {
            role: Role::new("system"),
            content: content(system)?,
            name: None,
            tool_call_id: None,
            tool_calls: Vec::new(),
        });
    }
    messages.extend(
        source
            .messages
            .iter()
            .map(message)
            .collect::<Result<Vec<_>, _>>()?,
    );
    let mut result = ModelRequest::new(source.model.as_ref(), messages);
    result.max_output_tokens = Some(source.max_tokens);
    result.stream = source.stream;
    result.tools = source.tools.iter().map(tool).collect::<Result<_, _>>()?;
    result.tool_choice = source
        .tool_choice
        .as_ref()
        .map(crate::decodechoice::anthropic);
    if let Some(thinking) = source.thinking.as_ref() {
        result.options.insert(
            "anthropic.thinking".to_owned(),
            serde_json::json!({ "type": thinking.kind, "budget_tokens": thinking.budget_tokens }),
        );
    }
    for (name, raw) in &source.extra {
        result.options.insert(
            name.to_string(),
            crate::semantic::raw_value(crate::WireProtocol::Anthropic, name, raw)?,
        );
    }
    result.context = ContextState::client_owned();
    Ok(result)
}

fn message(source: &crate::anthropic::Message<'_>) -> Result<Message, ConversionError> {
    let mut calls = Vec::new();
    if let NativeContent::Blocks(ref blocks) = source.content {
        for block in blocks {
            if let Block::ToolUse {
                ref id,
                ref name,
                input,
            } = *block
            {
                calls.push(ToolCall {
                    id: id.to_string(),
                    name: name.to_string(),
                    arguments: crate::semantic::raw_value(
                        crate::WireProtocol::Anthropic,
                        "messages[].content[].input",
                        input,
                    )?,
                });
            }
        }
    }
    Ok(Message {
        role: Role::new(source.role.as_ref()),
        content: content(&source.content)?,
        name: None,
        tool_call_id: None,
        tool_calls: calls,
    })
}

fn content(source: &NativeContent<'_>) -> Result<Content, ConversionError> {
    match *source {
        NativeContent::Text(ref text) => Ok(Content::text(text.as_ref())),
        NativeContent::Blocks(ref blocks) => Ok(Content::Parts(
            blocks
                .iter()
                .map(crate::decodeanthropicpart::part)
                .collect::<Result<_, _>>()?,
        )),
    }
}

fn tool(source: &crate::anthropic::Tool<'_>) -> Result<crate::ToolDefinition, ConversionError> {
    Ok(crate::ToolDefinition {
        name: source.name.to_string(),
        description: source.description.as_ref().map(ToString::to_string),
        parameters: crate::semantic::raw_value(
            crate::WireProtocol::Anthropic,
            "tools[].input_schema",
            source.input_schema,
        )?,
    })
}
