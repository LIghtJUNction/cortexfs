use crate::anthropic::{Block, Content as NativeContent, Request};
use crate::{
    Content, ContentPart, ContextState, ConversionError, Message, ModelRequest, Role, ToolCall,
};

pub(super) fn request(input: &[u8]) -> Result<ModelRequest, ConversionError> {
    let source: Request<'_> = crate::semantic::parse(crate::WireProtocol::Anthropic, input)?;
    let mut messages = Vec::new();
    if let Some(system) = source.system.as_ref() {
        messages.push(Message {
            role: Role::new("system"),
            content: content(system)?.0,
            name: None,
            tool_call_id: None,
            tool_calls: Vec::new(),
        });
    }
    messages.extend(source.messages.iter().map(message).collect::<Result<Vec<_>, _>>()?);
    let mut result = ModelRequest::new(source.model.as_ref(), messages);
    result.max_output_tokens = Some(source.max_tokens);
    result.stream = source.stream;
    result.tools = source.tools.iter().map(tool).collect::<Result<_, _>>()?;
    result.tool_choice = source.tool_choice.as_ref().map(crate::decodechoice::anthropic);
    if let Some(thinking) = source.thinking.as_ref() {
        result.options.insert(
            "anthropic.thinking".to_owned(),
            serde_json::json!({ "type": thinking.kind, "budget_tokens": thinking.budget_tokens }),
        );
    }
    for (name, raw) in &source.extra {
        let value = crate::semantic::raw_value(crate::WireProtocol::Anthropic, name, raw)?;
        result.options.insert(name.to_string(), value);
    }
    result.context = ContextState::client_owned();
    Ok(result)
}

fn message(source: &crate::anthropic::Message<'_>) -> Result<Message, ConversionError> {
    let (content, tool_calls) = content(&source.content)?;
    Ok(Message {
        role: Role::new(source.role.as_ref()),
        content,
        name: None,
        tool_call_id: None,
        tool_calls,
    })
}

fn content(source: &NativeContent<'_>) -> Result<(Content, Vec<ToolCall>), ConversionError> {
    let NativeContent::Blocks(blocks) = source else {
        let NativeContent::Text(text) = source else {
            unreachable!()
        };
        return Ok((Content::text(text.as_ref()), Vec::new()));
    };
    let mut parts = Vec::new();
    let mut calls = Vec::new();
    for block in blocks {
        match block {
            Block::Text { text } => parts.push(ContentPart::text(text.as_ref())),
            Block::Thinking {
                thinking,
                signature,
            } => parts.push(ContentPart::Data {
                name: "anthropic.thinking".to_owned(),
                value: serde_json::json!({"text": thinking, "signature": signature}),
            }),
            Block::ToolUse { id, name, input } => calls.push(ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                arguments: crate::semantic::raw_value(
                    crate::WireProtocol::Anthropic,
                    "messages[].content[].input",
                    input,
                )?,
            }),
            Block::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => parts.push(ContentPart::Data {
                name: format!("anthropic.tool_result:{tool_use_id}"),
                value: serde_json::json!({"content": content, "is_error": is_error}),
            }),
        }
    }
    Ok((Content::Parts(parts), calls))
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
