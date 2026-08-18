use std::fmt;

/// Registration or alias mapping failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetadataError {
    EmptyProvider,
    EmptyModelId,
    EmptyAlias,
    DuplicateModel(String),
    AliasConflict(String),
    UnknownModel(String),
}

impl fmt::Display for MetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::EmptyProvider => f.write_str("metadata provider is empty"),
            Self::EmptyModelId => f.write_str("metadata model id is empty"),
            Self::EmptyAlias => f.write_str("metadata alias is empty"),
            Self::DuplicateModel(ref key) => write!(f, "metadata model already exists: {key}"),
            Self::AliasConflict(ref alias) => write!(f, "metadata alias conflicts: {alias}"),
            Self::UnknownModel(ref key) => write!(f, "metadata model does not exist: {key}"),
        }
    }
}

impl std::error::Error for MetadataError {}
