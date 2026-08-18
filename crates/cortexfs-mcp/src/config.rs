use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    #[serde(rename = "mcpServers")]
    servers: BTreeMap<String, Server>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Server {
    pub(crate) command: String,
    #[serde(default)]
    pub(crate) args: Vec<String>,
    #[serde(default)]
    pub(crate) env: BTreeMap<String, String>,
}

pub(crate) fn read(path: &Path, name: &str, expected: Option<&str>) -> io::Result<Server> {
    let text = cortexfs::support::plain::read_small_text_file(path, MAX_CONFIG_BYTES)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    if expected.is_some_and(|expected| expected != digest(text.as_bytes())) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "MCP config digest mismatch",
        ));
    }
    let config: Config = serde_json::from_str(&text)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    config
        .servers
        .get(name)
        .cloned()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "MCP server not found"))
}

pub(crate) fn file_digest(path: &Path) -> io::Result<String> {
    let text = cortexfs::support::plain::read_small_text_file(path, MAX_CONFIG_BYTES)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    Ok(digest(text.as_bytes()))
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            use std::fmt::Write as _;
            let _ignored = write!(text, "{byte:02x}");
            text
        })
}
