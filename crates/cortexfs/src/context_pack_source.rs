use crate::{SESSION_REQUIRED_FILES, is_object_name};

/// Stable reason a context pack source is refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextPackSourceError {
    /// Source is empty.
    Empty,
    /// Source is absolute instead of relative to the owning session.
    Absolute,
    /// Source contains an empty path component.
    EmptyComponent,
    /// Source contains `.`.
    DotComponent,
    /// Source contains `..`.
    ParentComponent,
    /// Source names a child result path outside the allowed parent-owned result channel.
    UnsupportedChildPath,
    /// Source is neither a durable session file nor a `context/` path.
    UnsupportedSessionPath,
}

impl ContextPackSourceError {
    /// Returns a stable short reason string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Absolute => "absolute",
            Self::EmptyComponent => "empty component",
            Self::DotComponent => "dot component",
            Self::ParentComponent => "parent component",
            Self::UnsupportedChildPath => "unsupported child path",
            Self::UnsupportedSessionPath => "unsupported session path",
        }
    }
}

/// Validates a session-relative context-pack source path.
pub fn validate_context_pack_source(source: &str) -> Result<(), ContextPackSourceError> {
    if source.is_empty() {
        return Err(ContextPackSourceError::Empty);
    }
    if source.starts_with('/') {
        return Err(ContextPackSourceError::Absolute);
    }
    let parts = parse_session_relative_source(source)?;

    if parts.len() == 1 {
        return if parts
            .first()
            .is_some_and(|file| SESSION_REQUIRED_FILES.contains(file))
        {
            Ok(())
        } else {
            Err(ContextPackSourceError::UnsupportedSessionPath)
        };
    }
    if parts.first() != Some(&"context") {
        return Err(ContextPackSourceError::UnsupportedSessionPath);
    }
    if parts.len() == 2
        && parts.get(1).is_some_and(|file| {
            matches!(
                *file,
                "budget"
                    | "pack.json"
                    | "pack.md"
                    | "summary.md"
                    | "todo.md"
                    | "facts.jsonl"
                    | "decisions.jsonl"
                    | "refs.jsonl"
            )
        })
    {
        return Ok(());
    }
    if parts.len() == 3
        && parts
            .get(1)
            .is_some_and(|dir| matches!(*dir, "pinned" | "swap" | "dedup"))
    {
        return if parts.get(2).is_some_and(|name| is_object_name(name)) {
            Ok(())
        } else {
            Err(ContextPackSourceError::UnsupportedSessionPath)
        };
    }
    if parts.get(1) == Some(&"child") {
        return match (parts.get(2), parts.get(3..)) {
            (Some(child), Some(rest)) => validate_child_pack_source(child, rest),
            _ => Err(ContextPackSourceError::UnsupportedChildPath),
        };
    }

    Err(ContextPackSourceError::UnsupportedSessionPath)
}

fn parse_session_relative_source(source: &str) -> Result<Vec<&str>, ContextPackSourceError> {
    let mut parts = Vec::new();
    for part in source.split('/') {
        if part.is_empty() {
            return Err(ContextPackSourceError::EmptyComponent);
        }
        if part == "." {
            return Err(ContextPackSourceError::DotComponent);
        }
        if part == ".." {
            return Err(ContextPackSourceError::ParentComponent);
        }
        parts.push(part);
    }
    Ok(parts)
}

fn validate_child_pack_source(child: &str, rest: &[&str]) -> Result<(), ContextPackSourceError> {
    if !is_object_name(child) {
        return Err(ContextPackSourceError::UnsupportedChildPath);
    }
    if rest.len() == 1
        && rest
            .first()
            .is_some_and(|file| matches!(*file, "handoff.md" | "result.md" | "refs.jsonl"))
    {
        return Ok(());
    }
    if rest.len() == 2
        && rest.first() == Some(&"artifact")
        && rest.get(1).is_some_and(|name| is_object_name(name))
    {
        return Ok(());
    }
    Err(ContextPackSourceError::UnsupportedChildPath)
}
