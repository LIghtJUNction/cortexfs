use crate::{ConversionError, Message, ModelRequest, ToolChoice, WireProtocol};
use serde_json::{Map, Value, json};

pub(super) fn request(request: &ModelRequest) -> Result<Vec<u8>, ConversionError> {
    crate::encode::check_context(request, WireProtocol::OpenAiChat)?;
    let mut root = Map::new();
    root.insert("model".to_owned(), Value::String(request.model.clone()));
    root.insert(
        "messages".to_owned(),
        Value::Array(
            request
                .messages
                .iter()
                .map(message)
                .collect::<Result<_, _>>()?,
        ),
    );
    if !request.tools.is_empty() {
        root.insert(
            "tools".to_owned(),
            Value::Array(request.tools.iter().map(tool).collect()),
        );
    }
    if let Some(choice) = request.tool_choice.as_ref() {
        root.insert("tool_choice".to_owned(), choice_value(choice));
    }
    root.insert("stream".to_owned(), Value::Bool(request.stream));
    if let Some(tokens) = request.max_output_tokens {
        root.insert("max_tokens".to_owned(), json!(tokens));
    }
    crate::encode::options(&mut root, request);
    let value = Value::Object(root);
    crate::encode::bytes(WireProtocol::OpenAiChat, &value)
}

fn message(source: &Message) -> Result<Value, ConversionError> {
    let mut value = Map::new();
    value.insert(
        "role".to_owned(),
        Value::String(source.role.as_str().to_owned()),
    );
    value.insert(
        "content".to_owned(),
        crate::encode::text_or_parts(&source.content, "text", "image_url")?,
    );
    if let Some(name) = source.name.as_ref() {
        value.insert("name".to_owned(), Value::String(name.clone()));
    }
    if let Some(id) = source.tool_call_id.as_ref() {
        value.insert("tool_call_id".to_owned(), Value::String(id.clone()));
    }
    if !source.tool_calls.is_empty() {
        value.insert("tool_calls".to_owned(), Value::Array(source.tool_calls.iter().map(|call| json!({"id": call.id, "type": "function", "function": {"name": call.name, "arguments": call.arguments.to_string()}})).collect()));
    }
    Ok(Value::Object(value))
}

fn tool(source: &crate::ToolDefinition) -> Value {
    json!({"type": "function", "function": {"name": source.name, "description": source.description, "parameters": source.parameters}})
}

fn choice_value(choice: &ToolChoice) -> Value {
    match *choice {
        ToolChoice::Auto => Value::String("auto".to_owned()),
        ToolChoice::None => Value::String("none".to_owned()),
        ToolChoice::Required => Value::String("required".to_owned()),
        ToolChoice::Tool { ref name } => json!({"type": "function", "function": {"name": name}}),
    }
}
