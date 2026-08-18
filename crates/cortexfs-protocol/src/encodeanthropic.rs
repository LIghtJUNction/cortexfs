use crate::{Content, ConversionError, Message, ModelRequest, ToolChoice, WireProtocol};
use serde_json::{Map, Value, json};

pub(super) fn request(request: &ModelRequest) -> Result<Vec<u8>, ConversionError> {
    crate::encode::check_context(request, WireProtocol::Anthropic)?;
    let mut root = Map::new();
    root.insert("model".to_owned(), Value::String(request.model.clone()));
    root.insert(
        "max_tokens".to_owned(),
        json!(request.max_output_tokens.unwrap_or(4096)),
    );
    let systems = request
        .messages
        .iter()
        .filter(|message| message.role.as_str() == "system")
        .map(|message| message.content.text_value())
        .collect::<Vec<_>>();
    if !systems.is_empty() {
        root.insert("system".to_owned(), Value::String(systems.join("\n")));
    }
    root.insert(
        "messages".to_owned(),
        Value::Array(
            request
                .messages
                .iter()
                .filter(|message| message.role.as_str() != "system")
                .map(message)
                .collect::<Result<_, _>>()?,
        ),
    );
    if !request.tools.is_empty() {
        root.insert("tools".to_owned(), Value::Array(request.tools.iter().map(|tool| json!({"name": tool.name, "description": tool.description, "input_schema": tool.parameters})).collect()));
    }
    if let Some(tool_choice) = request.tool_choice.as_ref() {
        root.insert("tool_choice".to_owned(), choice(tool_choice));
    }
    if let Some(thinking) = request.options.get("anthropic.thinking") {
        root.insert("thinking".to_owned(), thinking.clone());
    }
    root.insert("stream".to_owned(), Value::Bool(request.stream));
    crate::encode::options(&mut root, request);
    let value = Value::Object(root);
    crate::encode::bytes(WireProtocol::Anthropic, &value)
}

fn message(source: &Message) -> Result<Value, ConversionError> {
    let role = if source.role.as_str() == "tool" {
        "user"
    } else {
        source.role.as_str()
    };
    let mut blocks = parts(&source.content)?;
    if source.role.as_str() == "tool" {
        blocks.push(json!({"type": "tool_result", "tool_use_id": source.tool_call_id, "content": source.content.text_value()}));
    }
    for call in &source.tool_calls {
        blocks.push(
            json!({"type": "tool_use", "id": call.id, "name": call.name, "input": call.arguments}),
        );
    }
    Ok(json!({"role": role, "content": blocks}))
}

fn parts(content: &Content) -> Result<Vec<Value>, ConversionError> {
    match *content {
        Content::Text(ref text) => Ok(vec![json!({"type": "text", "text": text})]),
        Content::Parts(ref parts) => parts
            .iter()
            .map(|part| match *part {
                crate::ContentPart::Text { ref text } => Ok(json!({"type": "text", "text": text})),
                crate::ContentPart::Data {
                    ref name,
                    ref value,
                } => Ok(json!({"type": "text", "text": format!("{name}: {value}")})),
                crate::ContentPart::Image { .. } | crate::ContentPart::Audio { .. } => {
                    Err(ConversionError::UnsupportedField {
                        protocol: WireProtocol::Anthropic,
                        field: "content part".to_owned(),
                    })
                }
            })
            .collect(),
    }
}

fn choice(choice: &ToolChoice) -> Value {
    match *choice {
        ToolChoice::Auto | ToolChoice::None => json!({"type": "auto"}),
        ToolChoice::Required => json!({"type": "any"}),
        ToolChoice::Tool { ref name } => json!({"type": "tool", "name": name}),
    }
}
