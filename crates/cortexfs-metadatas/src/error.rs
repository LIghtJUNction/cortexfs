use std::{fmt, io, path::PathBuf};

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

/// Errors from remote model metadata load and cache operations.
#[derive(Debug)]
pub enum MetadataSourceError {
    CacheMissing,
    CacheInvalid(PathBuf, String),
    CacheIoRead {
        path: PathBuf,
        source: io::Error,
    },
    CacheIoWrite {
        path: PathBuf,
        source: io::Error,
    },
    CacheCorrupt {
        path: PathBuf,
        source: serde_json::Error,
    },
    CacheOversize(usize),
    InvalidRemote(String),
    FetchFailed(String),
}

impl fmt::Display for MetadataSourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::CacheMissing => f.write_str("metadata cache is missing"),
            Self::CacheInvalid(_, ref reason) => write!(f, "invalid metadata cache: {reason}"),
            Self::CacheIoRead { ref path, .. } => {
                write!(f, "failed to read metadata cache file: {}", path.display())
            }
            Self::CacheIoWrite { ref path, .. } => {
                write!(f, "failed to write metadata cache file: {}", path.display())
            }
            Self::CacheCorrupt { ref path, .. } => {
                write!(f, "failed to parse metadata cache file: {}", path.display())
            }
            Self::CacheOversize(size) => write!(f, "metadata cache is too large: {size}"),
            Self::InvalidRemote(ref message) => write!(f, "invalid metadata response: {message}"),
            Self::FetchFailed(ref message) => write!(f, "metadata fetch failed: {message}"),
        }
    }
}

impl std::error::Error for MetadataSourceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            Self::CacheIoRead { ref source, .. } | Self::CacheIoWrite { ref source, .. } => {
                Some(source)
            }
            Self::CacheCorrupt { ref source, .. } => Some(source),
            _ => None,
        }
    }
}
