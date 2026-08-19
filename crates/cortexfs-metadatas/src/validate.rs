use crate::{ModelMetadata, Support};
use serde_json::Value;

pub use crate::validation::ModelsDevValidationError;
use crate::validation::{interleaved_support, optional_support, raw_modalities, required_u32};
#[must_use]
pub fn is_models_dev_record(metadata: &ModelMetadata) -> bool {
    metadata.sources.iter().any(|source| {
        source.publisher == "models.dev" && source.confidence == crate::SourceConfidence::Official
    })
}

pub fn validate_models_dev_record(
    metadata: &ModelMetadata,
) -> Result<(), ModelsDevValidationError> {
    let Some(raw) = metadata.models_dev.as_ref() else {
        return Err(ModelsDevValidationError::MissingPayload);
    };
    let Some(object) = raw.as_object() else {
        return Err(ModelsDevValidationError::NotObject);
    };
    for field in [
        "id",
        "name",
        "attachment",
        "reasoning",
        "tool_call",
        "modalities",
        "open_weights",
        "limit",
    ] {
        if !object.contains_key(field) {
            return Err(ModelsDevValidationError::MissingField(field));
        }
    }
    if object.get("id").and_then(Value::as_str) != Some(metadata.id.as_str()) {
        return Err(ModelsDevValidationError::IdentityMismatch);
    }
    let context = required_u32(raw, &["limit", "context"])?;
    if metadata.context_window_tokens != (context > 0).then_some(context) {
        return Err(ModelsDevValidationError::ContextMismatch);
    }
    let output = required_u32(raw, &["limit", "output"])?;
    if metadata.max_output_tokens != (output > 0).then_some(output) {
        return Err(ModelsDevValidationError::OutputMismatch);
    }
    for (field, support) in [
        ("attachment", metadata.attachment),
        ("open_weights", metadata.open_weights),
        ("tool_call", metadata.tools),
        ("reasoning", metadata.reasoning.support),
    ] {
        let expected = match object.get(field).and_then(Value::as_bool) {
            Some(true) => Support::Supported,
            Some(false) => Support::Unsupported,
            None => return Err(ModelsDevValidationError::InvalidField(field)),
        };
        if support != expected {
            return Err(ModelsDevValidationError::CapabilityMismatch(field));
        }
    }
    for (field, support) in [
        ("structured_output", metadata.structured_output),
        ("streaming", metadata.streaming),
        ("temperature", metadata.temperature),
    ] {
        if object.contains_key(field) && optional_support(raw, field)? != support {
            return Err(ModelsDevValidationError::CapabilityMismatch(field));
        }
    }
    if object.contains_key("interleaved") && interleaved_support(raw) != metadata.interleaved {
        return Err(ModelsDevValidationError::CapabilityMismatch("interleaved"));
    }
    let modalities = object
        .get("modalities")
        .and_then(Value::as_object)
        .ok_or(ModelsDevValidationError::InvalidField("modalities"))?;
    for field in ["input", "output"] {
        if !modalities.get(field).is_some_and(Value::is_array) {
            return Err(ModelsDevValidationError::InvalidField(field));
        }
    }
    if raw_modalities(raw, "input")? != metadata.input_modalities
        || raw_modalities(raw, "output")? != metadata.output_modalities
    {
        return Err(ModelsDevValidationError::CapabilityMismatch("modalities"));
    }
    Ok(())
}
