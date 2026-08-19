use crate::{Modality, Support};
use serde_json::Value;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelsDevValidationError {
    MissingPayload,
    NotObject,
    MissingField(&'static str),
    InvalidField(&'static str),
    IdentityMismatch,
    ContextMismatch,
    OutputMismatch,
    CapabilityMismatch(&'static str),
}

impl fmt::Display for ModelsDevValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::MissingPayload => f.write_str("models.dev payload is missing"),
            Self::NotObject => f.write_str("models.dev payload is not an object"),
            Self::MissingField(field) => write!(f, "models.dev field is missing: {field}"),
            Self::InvalidField(field) => write!(f, "models.dev field is invalid: {field}"),
            Self::IdentityMismatch => f.write_str("models.dev model id does not match metadata"),
            Self::ContextMismatch => {
                f.write_str("models.dev context limit does not match metadata")
            }
            Self::OutputMismatch => f.write_str("models.dev output limit does not match metadata"),
            Self::CapabilityMismatch(field) => {
                write!(f, "models.dev capability does not match metadata: {field}")
            }
        }
    }
}

impl std::error::Error for ModelsDevValidationError {}

pub(super) fn optional_support(
    value: &Value,
    field: &'static str,
) -> Result<Support, ModelsDevValidationError> {
    let Some(value) = value.get(field) else {
        return Ok(Support::Unknown);
    };
    if value.is_null() {
        return Ok(Support::Unknown);
    }
    match value.as_bool() {
        Some(true) => Ok(Support::Supported),
        Some(false) => Ok(Support::Unsupported),
        None => Err(ModelsDevValidationError::InvalidField(field)),
    }
}

pub(super) fn interleaved_support(value: &Value) -> Support {
    let Some(value) = value.get("interleaved") else {
        return Support::Unknown;
    };
    if value.is_null() {
        Support::Unknown
    } else if value.as_bool() == Some(false) {
        Support::Unsupported
    } else {
        Support::Supported
    }
}

pub(super) fn raw_modalities(
    value: &Value,
    field: &'static str,
) -> Result<Vec<Modality>, ModelsDevValidationError> {
    let values = value
        .get("modalities")
        .and_then(Value::as_object)
        .and_then(|value| value.get(field))
        .and_then(Value::as_array)
        .ok_or(ModelsDevValidationError::InvalidField(field))?;
    let mut parsed = Vec::new();
    for value in values {
        let value = value
            .as_str()
            .ok_or(ModelsDevValidationError::InvalidField(field))?;
        if let Some(modality) = parse_modality(value) {
            parsed.push(modality);
        }
    }
    Ok(parsed)
}

fn parse_modality(value: &str) -> Option<Modality> {
    match value {
        "text" => Some(Modality::Text),
        "image" => Some(Modality::Image),
        "audio" => Some(Modality::Audio),
        "video" => Some(Modality::Video),
        "pdf" => Some(Modality::Pdf),
        "embedding" => Some(Modality::Embedding),
        _ => None,
    }
}

pub(super) fn required_u32(
    value: &Value,
    path: &[&'static str],
) -> Result<u32, ModelsDevValidationError> {
    let Some(value) = path
        .iter()
        .try_fold(value, |value, field| value.get(*field))
    else {
        return Err(ModelsDevValidationError::MissingField(
            path.last().copied().unwrap_or("value"),
        ));
    };
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| {
            ModelsDevValidationError::InvalidField(path.last().copied().unwrap_or("value"))
        })
}
