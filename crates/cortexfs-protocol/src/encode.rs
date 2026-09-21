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
