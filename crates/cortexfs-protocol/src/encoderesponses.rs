use crate::{Content, ContextReference, ConversionError, Message, ModelRequest, WireProtocol};
use serde_json::{Map, Value, json};

pub(super) fn request(request: &ModelRequest) -> Result<Vec<u8>, ConversionError> {
    crate::encode::check_context(request, WireProtocol::OpenAiResponses)?;
    let mut root = Map::new();
    root.insert("model".to_owned(), Value::String(request.model.clone()));
    let systems = request
        .messages
        .iter()
        .filter(|message| message.role.as_str() == "system")
        .collect::<Vec<_>>();
    if !systems.is_empty() {
        root.insert(
            "instructions".to_owned(),
            Value::String(
                systems
                    .iter()
                    .map(|message| message.content.text_value())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        );
    }
    let input = request
        .messages
        .iter()
        .filter(|message| message.role.as_str() != "system")
        .map(items)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect();
    root.insert("input".to_owned(), Value::Array(input));
    if !request.tools.is_empty() {
        root.insert("tools".to_owned(), Value::Array(request.tools.iter().map(|tool| json!({"type": "function", "name": tool.name, "description": tool.description, "parameters": tool.parameters})).collect()));
    }
    root.insert("stream".to_owned(), Value::Bool(request.stream));
    if let Some(tokens) = request.max_output_tokens {
        root.insert("max_output_tokens".to_owned(), json!(tokens));
    }
    if let Some(reference) = request.context.reference.as_ref() {
        context(&mut root, reference)?;
    }
    crate::encode::options(&mut root, request);
    let value = Value::Object(root);
    crate::encode::bytes(WireProtocol::OpenAiResponses, &value)
}

fn context(
    root: &mut Map<String, Value>,
    reference: &ContextReference,
) -> Result<(), ConversionError> {
    if reference.namespace == "openai.responses.previous_response_id" {
        root.insert(
            "previous_response_id".to_owned(),
            Value::String(reference.value.clone()),
        );
        return Ok(());
    }
    if reference.namespace == "openai.responses.conversation" {
        root.insert(
            "conversation".to_owned(),
            Value::String(reference.value.clone()),
        );
        return Ok(());
    }
    Err(ConversionError::UnsupportedField {
        protocol: WireProtocol::OpenAiResponses,
        field: "foreign context reference".to_owned(),
    })
}

fn items(source: &Message) -> Result<Vec<Value>, ConversionError> {
    let mut values = Vec::new();
    if source.role.as_str() == "tool" {
        values.push(json!({"type": "function_call_output", "call_id": source.tool_call_id, "output": source.content.text_value()}));
        return Ok(values);
    }
    values.push(json!({"type": "message", "role": source.role.as_str(), "content": parts(&source.content, source.role.as_str())?}));
    for call in &source.tool_calls {
        values.push(json!({"type": "function_call", "call_id": call.id, "name": call.name, "arguments": call.arguments.to_string()}));
    }
    Ok(values)
}

fn parts(content: &Content, role: &str) -> Result<Vec<Value>, ConversionError> {
    match *content {
        Content::Text(ref text) => Ok(vec![json!({"type": if role == "assistant" { "output_text" } else { "input_text" }, "text": text})]),
        Content::Parts(ref parts) => parts.iter().map(|part| match *part {
            crate::ContentPart::Text { ref text } => Ok(json!({"type": if role == "assistant" { "output_text" } else { "input_text" }, "text": text})),
            crate::ContentPart::Image { ref uri, .. } => Ok(json!({"type": "image_url", "image_url": uri})),
            crate::ContentPart::Audio { .. } | crate::ContentPart::Data { .. } => Err(ConversionError::UnsupportedField { protocol: WireProtocol::OpenAiResponses, field: "content part".to_owned() }),
        }).collect(),
    }
}
