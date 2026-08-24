use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::support::atomic::atomic_replace_text_with_mode;
use crate::support::plain::{CreatePlainDirMessages, create_plain_dir_with, read_small_text_file};

const TSH_CONTEXT_STATE_VERSION: u32 = 1;
const MAX_TSH_CONTEXT_STATE_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[expect(
    clippy::partial_pub_fields,
    reason = "state version is an internal schema guard; loaded tool entries are public ABI data"
)]
pub struct TshContextState {
    #[serde(default = "default_tsh_context_state_version")]
    version: u32,
    #[serde(default)]
    pub tools: Vec<TshLoadedToolState>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TshLoadedToolState {
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub schema: Option<String>,
    #[serde(default)]
    pub dynamic_resident: bool,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub last_used: u64,
}

#[must_use]
pub fn tsh_context_state_path(session_dir: &Path) -> PathBuf {
    session_dir.join("context").join("tsh.json")
}

/// Returns whether a session working set contains one direct tool.
pub fn tsh_context_contains(path: &Path, name: &str) -> io::Result<bool> {
    Ok(read_tsh_context_state(path)?
        .tools
        .iter()
        .any(|tool| tool.name == name))
}

/// Records one successful direct tool use and evicts the least-recently-used
/// unpinned entry when the session working set is full.
pub fn retain_tsh_context_tool(
    path: &Path,
    mut tool: TshLoadedToolState,
    max_loaded_tools: usize,
) -> io::Result<()> {
    let mut state = read_tsh_context_state(path)?;
    let clock = state
        .tools
        .iter()
        .map(|entry| entry.last_used)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    if let Some(existing) = state.tools.iter_mut().find(|entry| entry.name == tool.name) {
        existing.last_used = clock;
    } else {
        tool.last_used = clock;
        state.tools.push(tool);
    }
    let limit = max_loaded_tools.max(1);
    while state.tools.iter().filter(|entry| !entry.pinned).count() > limit {
        let Some(index) = state
            .tools
            .iter()
            .enumerate()
            .filter(|&(_, entry)| !entry.pinned)
            .min_by_key(|&(_, entry)| (entry.last_used, entry.name.as_str()))
            .map(|(index, _)| index)
        else {
            break;
        };
        state.tools.remove(index);
    }
    write_tsh_context_state(path, &state)
}

pub fn read_tsh_context_state(path: &Path) -> io::Result<TshContextState> {
    let content = match read_small_text_file(path, MAX_TSH_CONTEXT_STATE_BYTES) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(TshContextState::default());
        }
        Err(error) => return Err(error),
    };
    let state = serde_json::from_str::<TshContextState>(&content)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if state.version != TSH_CONTEXT_STATE_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported tsh context state version",
        ));
    }
    Ok(state)
}

pub fn write_tsh_context_state(path: &Path, state: &TshContextState) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing state parent"))?;
    create_plain_dir_with(parent, CreatePlainDirMessages::library_defaults())?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid tsh context state file",
            ));
        }
        Ok(_metadata) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut content = serde_json::to_string_pretty(state)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    content.push('\n');
    atomic_replace_text_with_mode(path, &content, 0o600)
}

pub(crate) fn default_tsh_context_state_version() -> u32 {
    TSH_CONTEXT_STATE_VERSION
}

impl Default for TshContextState {
    fn default() -> Self {
        Self {
            version: TSH_CONTEXT_STATE_VERSION,
            tools: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn loaded(name: &str, pinned: bool) -> TshLoadedToolState {
        TshLoadedToolState {
            name: name.to_owned(),
            path: PathBuf::from("/ctx/tool").join(name),
            description: String::new(),
            schema: None,
            dynamic_resident: true,
            pinned,
            last_used: 0,
        }
    }

    #[test]
    fn session_working_sets_are_isolated_and_resume_from_disk() {
        let root =
            std::env::temp_dir().join(format!("cortexfs-tsh-session-state-{}", std::process::id()));
        let _ignored = fs::remove_dir_all(&root);
        let session_a = tsh_context_state_path(&root.join("session-a"));
        let session_b = tsh_context_state_path(&root.join("session-b"));

        assert!(retain_tsh_context_tool(&session_a, loaded("bash", false), 8).is_ok());

        assert!(matches!(tsh_context_contains(&session_a, "bash"), Ok(true)));
        assert!(matches!(
            tsh_context_contains(&session_b, "bash"),
            Ok(false)
        ));
        assert_eq!(
            read_tsh_context_state(&session_a)
                .ok()
                .and_then(|state| state.tools.first().map(|tool| tool.name.clone())),
            Some("bash".to_owned())
        );
        let _ignored = fs::remove_dir_all(root);
    }

    #[test]
    fn successful_use_touches_and_limit_evicts_only_unpinned_lru() {
        let root =
            std::env::temp_dir().join(format!("cortexfs-tsh-session-lru-{}", std::process::id()));
        let _ignored = fs::remove_dir_all(&root);
        let path = tsh_context_state_path(&root);

        assert!(retain_tsh_context_tool(&path, loaded("pinned", true), 1).is_ok());
        assert!(retain_tsh_context_tool(&path, loaded("old", false), 1).is_ok());
        assert!(retain_tsh_context_tool(&path, loaded("old", false), 1).is_ok());
        assert!(retain_tsh_context_tool(&path, loaded("new", false), 1).is_ok());

        let state = read_tsh_context_state(&path).unwrap_or_default();
        assert!(
            state
                .tools
                .iter()
                .any(|tool| tool.name == "pinned" && tool.pinned)
        );
        assert!(state.tools.iter().any(|tool| tool.name == "new"));
        assert!(!state.tools.iter().any(|tool| tool.name == "old"));
        let _ignored = fs::remove_dir_all(root);
    }

    #[test]
    fn state_io_rejects_symlink_target_and_parent_without_touching_outside() {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-tsh-session-symlink-{}",
            std::process::id()
        ));
        let _ignored = fs::remove_dir_all(&root);
        let outside = root.join("outside");
        assert!(fs::create_dir_all(&outside).is_ok());
        let outside_file = outside.join("state");
        assert!(fs::write(&outside_file, "outside\n").is_ok());

        let session = root.join("session");
        assert!(fs::create_dir_all(session.join("context")).is_ok());
        let state = tsh_context_state_path(&session);
        assert!(symlink(&outside_file, &state).is_ok());
        assert!(write_tsh_context_state(&state, &TshContextState::default()).is_err());
        assert_eq!(
            fs::read_to_string(&outside_file).ok().as_deref(),
            Some("outside\n")
        );

        assert!(fs::remove_file(&state).is_ok());
        assert!(fs::remove_dir(session.join("context")).is_ok());
        assert!(symlink(&outside, session.join("context")).is_ok());
        assert!(write_tsh_context_state(&state, &TshContextState::default()).is_err());
        assert_eq!(
            fs::read_to_string(&outside_file).ok().as_deref(),
            Some("outside\n")
        );
        let _ignored = fs::remove_dir_all(root);
    }

    #[test]
    fn state_writer_ignores_legacy_predictable_temp_symlink() {
        let root =
            std::env::temp_dir().join(format!("cortexfs-tsh-session-temp-{}", std::process::id()));
        let _ignored = fs::remove_dir_all(&root);
        let path = tsh_context_state_path(&root);
        assert!(fs::create_dir_all(path.parent().unwrap_or(&root)).is_ok());
        let outside = root.join("outside");
        assert!(fs::write(&outside, "outside\n").is_ok());
        let legacy_tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
        assert!(symlink(&outside, &legacy_tmp).is_ok());

        assert!(write_tsh_context_state(&path, &TshContextState::default()).is_ok());
        assert_eq!(
            fs::read_to_string(&outside).ok().as_deref(),
            Some("outside\n")
        );
        assert!(fs::symlink_metadata(&legacy_tmp).is_ok_and(|meta| meta.file_type().is_symlink()));
        let _ignored = fs::remove_dir_all(root);
    }
}
