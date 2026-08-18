use crate::{ContextOwnership, ConversionError, ModelRequest, WireProtocol};
use serde_json::{Map, Value};

pub(super) fn check_context(
    request: &ModelRequest,
    target: WireProtocol,
) -> Result<(), ConversionError> {
    if request.context.reference.is_none() {
        return Ok(());
    }
    if target == WireProtocol::OpenAiResponses
        && matches!(
            request.context.ownership,
            ContextOwnership::ProviderOwned | ContextOwnership::Hybrid
        )
    {
        return Ok(());
    }
    Err(ConversionError::UnsupportedField {
        protocol: target,
        field: "context.reference cannot cross provider dialects".to_owned(),
    })
}

pub(super) fn bytes(protocol: WireProtocol, value: &Value) -> Result<Vec<u8>, ConversionError> {
    serde_json::to_vec(&value).map_err(|error| ConversionError::InvalidJson {
        protocol,
        detail: error.to_string(),
    })
}

pub(super) fn options(root: &mut Map<String, Value>, request: &ModelRequest) {
    for (name, value) in &request.options {
        if name == "anthropic.thinking" || name == "gemini.thinking_config" {
            continue;
        }
        root.entry(name.clone()).or_insert_with(|| value.clone());
    }
}

pub(super) fn text_or_parts(
    content: &crate::Content,
    text_type: &str,
    image_type: &str,
) -> Result<Value, ConversionError> {
    match *content {
        crate::Content::Text(ref text) => Ok(Value::String(text.clone())),
        crate::Content::Parts(ref parts) => parts
            .iter()
            .map(|part| match *part {
                crate::ContentPart::Text { ref text } => {
                    Ok(serde_json::json!({ "type": text_type, "text": text }))
                }
                crate::ContentPart::Image { ref uri, .. } => {
                    Ok(serde_json::json!({ "type": image_type, "image_url": uri }))
                }
                crate::ContentPart::Audio { .. } | crate::ContentPart::Data { .. } => {
                    Err(ConversionError::UnsupportedField {
                        protocol: WireProtocol::OpenAiChat,
                        field: "content part".to_owned(),
                    })
                }
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
    }
}
