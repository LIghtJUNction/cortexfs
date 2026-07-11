use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
pub fn tsh_context_state_path(agent_home: &Path) -> PathBuf {
    agent_home.join("cache").join("tsh").join("context.json")
}

pub fn read_tsh_context_state(path: &Path) -> io::Result<TshContextState> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(TshContextState::default());
        }
        Err(error) => return Err(error),
    };
    if !metadata.is_file() || metadata.len() > MAX_TSH_CONTEXT_STATE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid tsh context state file",
        ));
    }
    let content = fs::read_to_string(path)?;
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
    fs::create_dir_all(parent)?;
    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    let content = serde_json::to_string_pretty(state)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(content.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    if let Ok(parent_dir) = fs::File::open(parent) {
        parent_dir.sync_all()?;
    }
    Ok(())
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
