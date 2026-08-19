use super::ChannelRecord;
use crate::*;
use serde_json::Value;

pub(super) fn read_channel(
    path: &Path,
    default_agent: &str,
    shared: bool,
) -> Option<ChannelRecord> {
    let value = serde_json::from_str::<Value>(&fs::read_to_string(path).ok()?).ok()?;
    let object = value.as_object()?;
    let session = value.get("session")?.as_str()?.to_owned();
    let agent = value
        .get("agent")
        .and_then(Value::as_str)
        .unwrap_or(default_agent)
        .to_owned();
    let name = path.file_name()?.to_str()?.to_owned();
    if object.get("version").and_then(Value::as_u64) != Some(1)
        || object.get("name").and_then(Value::as_str) != Some(name.as_str())
        || !is_object_name(&agent)
        || !is_object_name(&session)
        || !matches!(
            object.get("scope").and_then(Value::as_str),
            Some("private" | "shared")
        )
    {
        return None;
    }
    let transport = value
        .get("transport")
        .and_then(Value::as_str)
        .or_else(|| value.get("endpoint").and_then(Value::as_str))
        .unwrap_or("channel")
        .to_owned();
    let session_dir = path.parent()?.parent()?.parent()?.join(&session);
    Some(ChannelRecord {
        name,
        agent,
        session,
        transport,
        state: session_state(&session_dir),
        shared: shared || value.get("scope").and_then(Value::as_str) == Some("shared"),
    })
}

pub(super) fn session_state(path: &Path) -> String {
    read_small_plain_text_file(&path.join("state"), 128, "session state")
        .map(|value| value.trim().to_owned())
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

pub(super) fn plain_directory(entry: &fs::DirEntry) -> bool {
    entry
        .file_type()
        .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
}

pub(super) fn plain_file(entry: &fs::DirEntry) -> bool {
    #[expect(
        clippy::filetype_is_file,
        reason = "channel indexes must accept regular files and reject symlinks"
    )]
    entry
        .file_type()
        .is_ok_and(|kind| kind.is_file() && !kind.is_symlink())
}
