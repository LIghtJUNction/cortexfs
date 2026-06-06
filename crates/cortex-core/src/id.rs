use crate::{ValidationError, ValidationReason};
use core::fmt;

const MAX_ID_LEN: usize = 128;
const MAX_FINGERPRINT_LEN: usize = 256;

macro_rules! stable_id {
    ($name:ident, $field:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
                let value = value.into();
                validate_id($field, &value)?;
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

stable_id!(
    ProviderId,
    "provider_id",
    "Logical provider identity, separate from API format and base URL."
);
stable_id!(
    SpaceId,
    "space_id",
    "Security and tenancy boundary for API objects, threads, memory, and audit."
);
stable_id!(
    ThreadId,
    "thread_id",
    "Stable thread identity inside a space."
);

/// Provider-local model identity.
#[derive(Debug, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ModelId(String);

impl ModelId {
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        validate_model_id(&value)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ModelId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Canonical request/context hash used for cache keys and audit correlation.
#[derive(Debug, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Fingerprint(String);

impl Fingerprint {
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        validate_fingerprint(&value)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Fingerprint {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn validate_id(field: &'static str, value: &str) -> Result<(), ValidationError> {
    validate_identifier_shape(field, value, is_id_byte)
}

fn validate_model_id(value: &str) -> Result<(), ValidationError> {
    validate_identifier_shape("model_id", value, is_model_id_byte)
}

fn validate_identifier_shape(
    field: &'static str,
    value: &str,
    is_allowed: impl Fn(u8) -> bool,
) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Err(ValidationError::invalid_identifier(
            field,
            ValidationReason::Empty,
            value,
        ));
    }

    if value.len() > MAX_ID_LEN {
        return Err(ValidationError::invalid_identifier(
            field,
            ValidationReason::TooLong,
            value,
        ));
    }

    if has_invalid_boundary(value) {
        return Err(ValidationError::invalid_identifier(
            field,
            ValidationReason::InvalidBoundary,
            value,
        ));
    }

    if !value.bytes().all(is_allowed) {
        return Err(ValidationError::invalid_identifier(
            field,
            ValidationReason::InvalidCharacter,
            value,
        ));
    }

    Ok(())
}

fn validate_fingerprint(value: &str) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Err(ValidationError::invalid_identifier(
            "fingerprint",
            ValidationReason::Empty,
            value,
        ));
    }

    if value.len() > MAX_FINGERPRINT_LEN {
        return Err(ValidationError::invalid_identifier(
            "fingerprint",
            ValidationReason::TooLong,
            value,
        ));
    }

    if !value.bytes().all(is_fingerprint_byte) {
        return Err(ValidationError::invalid_identifier(
            "fingerprint",
            ValidationReason::InvalidCharacter,
            value,
        ));
    }

    Ok(())
}

const fn has_invalid_boundary(value: &str) -> bool {
    let bytes = value.as_bytes();
    matches!(bytes.first(), Some(b'.' | b'-' | b'_'))
        || matches!(bytes.last(), Some(b'.' | b'-' | b'_'))
}

const fn is_id_byte(byte: u8) -> bool {
    matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'_')
}

const fn is_model_id_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'_' | b':'
    )
}

const fn is_fingerprint_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b':' | b'-' | b'_'
    )
}

#[cfg(test)]
mod tests {
    use super::{Fingerprint, ModelId, ProviderId, SpaceId, ThreadId};
    use crate::ValidationReason;

    #[test]
    fn ids_accept_stable_filesystem_names() {
        assert_eq!(
            ProviderId::new("kimi").map(|id| id.to_string()),
            Ok("kimi".to_owned())
        );
        assert_eq!(
            ModelId::new("moonshot-v1-32k").map(|id| id.to_string()),
            Ok("moonshot-v1-32k".to_owned())
        );
        assert_eq!(
            ModelId::new("smollm2:135m").map(|id| id.to_string()),
            Ok("smollm2:135m".to_owned())
        );
        assert_eq!(
            SpaceId::new("users.1000").map(|id| id.to_string()),
            Ok("users.1000".to_owned())
        );
        assert_eq!(
            ThreadId::new("design-review_1").map(|id| id.to_string()),
            Ok("design-review_1".to_owned())
        );
    }

    #[test]
    fn ids_reject_unsafe_names() {
        assert_eq!(
            ProviderId::new("BadProvider").map_err(|error| error.reason()),
            Err(ValidationReason::InvalidCharacter)
        );
        assert_eq!(
            ProviderId::new("-kimi").map_err(|error| error.reason()),
            Err(ValidationReason::InvalidBoundary)
        );
        assert_eq!(
            ProviderId::new("ollama:local").map_err(|error| error.reason()),
            Err(ValidationReason::InvalidCharacter)
        );
        assert_eq!(
            ProviderId::new("").map_err(|error| error.reason()),
            Err(ValidationReason::Empty)
        );
    }

    #[test]
    fn fingerprint_accepts_algorithm_prefix() {
        assert_eq!(
            Fingerprint::new("blake3:abc123").map(|id| id.to_string()),
            Ok("blake3:abc123".to_owned())
        );
        assert!(Fingerprint::new("blake3/abc123").is_err());
    }
}
