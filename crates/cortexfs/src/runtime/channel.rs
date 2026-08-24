//! Durable, provider-neutral channel index entries for session discovery.

use std::path::{Path, PathBuf};

use cortexfs_runtime_client::interaction::InteractionOrigin;

use crate::support::atomic::{
    atomic_replace_text_preserving_metadata, atomic_replace_text_with_mode,
};
use crate::support::plain::{CreatePlainDirMessages, create_plain_dir_with};
use crate::{SocketSessionScope, is_object_name};

const MAX_CHANNEL_NAME: usize = 64;

/// Returns the stable filename used for one attachable frontend.
#[must_use]
pub fn canonical_channel_name(
    origin: &InteractionOrigin,
    agent: &str,
    session: &str,
    scope: SocketSessionScope,
) -> String {
    let source = origin
        .endpoint
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            if origin.transport.is_empty() {
                "channel"
            } else {
                &origin.transport
            }
        });
    let mut parts = Vec::new();
    if scope == SocketSessionScope::Shared {
        parts.push("shared".to_owned());
    }
    parts.extend([normalize(source), normalize(agent), normalize(session)]);
    fit_name(
        parts
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("_"),
    )
}

/// Creates or replaces the channel filename for a durable session.
pub fn register_channel(
    session_root: &Path,
    session: &str,
    scope: SocketSessionScope,
    origin: &InteractionOrigin,
) -> std::io::Result<PathBuf> {
    if scope == SocketSessionScope::Temp || !is_object_name(session) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "channel session is not durable",
        ));
    }
    let index = cortexfs_paths::session_channel_index_path(session_root);
    create_plain_dir_with(&index, CreatePlainDirMessages::library_defaults())?;
    let agent = session_root
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or("agent");
    let name = canonical_channel_name(origin, agent, session, scope);
    let path = cortexfs_paths::session_channel_path(session_root, &name);
    let record = serde_json::json!({
        "version": 1,
        "name": name,
        "agent": agent,
        "session": session,
        "scope": scope.as_str(),
        "transport": origin.transport,
        "endpoint": origin.endpoint,
        "conversation": origin.conversation,
        "thread": origin.thread,
    });
    let content = format!("{record}\n");
    let mode = if scope == SocketSessionScope::Shared {
        0o640
    } else {
        0o600
    };
    if path.exists() {
        atomic_replace_text_preserving_metadata(&path, &content)?;
    } else {
        atomic_replace_text_with_mode(&path, &content, mode)?;
    }
    Ok(path)
}

fn normalize(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() {
            output.push(char::from(byte.to_ascii_lowercase()));
        } else if !output.ends_with('_') {
            output.push('_');
        }
    }
    output.trim_matches('_').to_owned()
}

fn fit_name(mut value: String) -> String {
    if value.len() <= MAX_CHANNEL_NAME {
        return value;
    }
    let hash = value.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    value.truncate(MAX_CHANNEL_NAME - 17);
    format!("{value}_{hash:016x}")
}
