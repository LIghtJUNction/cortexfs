use crate::anthropic::{Block, Content as NativeContent, Request};
use crate::decodechoice::anthropic as choice;
use crate::semantic::raw_value;
use crate::{
    Content, ContentPart, ContextState, ConversionError, Message, ModelRequest, Role, ToolCall,
    WireProtocol,
};

pub(super) fn request(input: &[u8]) -> Result<ModelRequest, ConversionError> {
    let source: Request<'_> = crate::semantic::parse(WireProtocol::Anthropic, input)?;
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
    for source_message in &source.messages {
        messages.push(message(source_message)?);
    }
    let mut result = ModelRequest::new(source.model.as_ref(), messages);
    result.max_output_tokens = Some(source.max_tokens);
    result.stream = source.stream;
    result.tools = source.tools.iter().map(tool).collect::<Result<_, _>>()?;
    result.tool_choice = source.tool_choice.as_ref().map(choice);
    if let Some(thinking) = source.thinking.as_ref() {
        result.options.insert(
            "anthropic.thinking".to_owned(),
            serde_json::json!({ "type": thinking.kind, "budget_tokens": thinking.budget_tokens }),
        );
    }
    for (name, raw) in &source.extra {
        result
            .options
            .insert(name.to_string(), raw_value(WireProtocol::Anthropic, name, raw)?);
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
    let blocks = match *source {
        NativeContent::Text(ref text) => return Ok((Content::text(text.as_ref()), Vec::new())),
        NativeContent::Blocks(ref blocks) => blocks,
    };
    let mut parts = Vec::new();
    let mut calls = Vec::new();
    for block in blocks {
        match *block {
            Block::Text { ref text } => parts.push(ContentPart::text(text.as_ref())),
            Block::Thinking {
                ref thinking,
                ref signature,
            } => parts.push(ContentPart::Data {
                name: "anthropic.thinking".to_owned(),
                value: serde_json::json!({"text": thinking, "signature": signature}),
            }),
            Block::ToolUse {
                ref id,
                ref name,
                input,
            } => calls.push(ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                arguments: raw_value(WireProtocol::Anthropic, "messages[].content[].input", input)?,
            }),
            Block::ToolResult {
                ref tool_use_id,
                ref content,
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
        parameters: raw_value(
            WireProtocol::Anthropic,
            "tools[].input_schema",
            source.input_schema,
        )?,
    })
}
