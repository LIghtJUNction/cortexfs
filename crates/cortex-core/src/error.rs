use core::fmt;

/// Validation failure for stable ABI/domain values.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ValidationError {
    field: &'static str,
    reason: ValidationReason,
    value: String,
}

impl ValidationError {
    #[must_use]
    pub fn field(&self) -> &'static str {
        self.field
    }

    #[must_use]
    pub const fn reason(&self) -> ValidationReason {
        self.reason
    }

    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    #[must_use]
    pub fn unsupported_api_format(value: &str) -> Self {
        Self {
            field: "api_format",
            reason: ValidationReason::UnsupportedValue,
            value: value.to_owned(),
        }
    }

    #[must_use]
    pub fn unsupported_message_role(value: &str) -> Self {
        Self {
            field: "message_role",
            reason: ValidationReason::UnsupportedValue,
            value: value.to_owned(),
        }
    }

    #[must_use]
    pub fn invalid_identifier(field: &'static str, reason: ValidationReason, value: &str) -> Self {
        Self {
            field,
            reason,
            value: value.to_owned(),
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid {} {:?}: {}",
            self.field, self.reason, self.value
        )
    }
}

impl std::error::Error for ValidationError {}

/// Machine-readable validation reason.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ValidationReason {
    Empty,
    TooLong,
    InvalidCharacter,
    InvalidBoundary,
    UnsupportedValue,
}
