use std::path::{Path, PathBuf};

use crate::cli::nofollow::open_executable_no_follow;
use crate::support::plain::read_small_text_file;
use crate::tool::invokestrategy::InvokeStrategy;

const MAX_STRATEGY_BYTES: u64 = 256;

/// Reads `tool/<name>.d/invoke.strategy`, defaulting to host-selected mode.
#[must_use]
pub fn read_invoke_strategy(control_dir: &Path) -> InvokeStrategy {
    match read_small_text_file(&control_dir.join("invoke.strategy"), MAX_STRATEGY_BYTES) {
        Ok(content) => InvokeStrategy::parse(&content).unwrap_or_default(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => InvokeStrategy::default(),
        Err(_error) => InvokeStrategy::default(),
    }
}

/// Returns the executable used for one authorized tool call.
#[must_use]
pub fn resolve_tool_invoke_executable(control_dir: &Path, default: &Path) -> PathBuf {
    let strategy = read_invoke_strategy(control_dir);
    let InvokeStrategy::Custom(name) = strategy else {
        return default.to_path_buf();
    };
    let custom = control_dir.join("invoke.d").join(&name);
    if open_executable_no_follow(&custom).is_ok() {
        custom
    } else {
        default.to_path_buf()
    }
}

/// Returns the `CTX_TOOL_MODE` value implied by one invoke strategy.
#[must_use]
pub fn invoke_tool_mode(strategy: &InvokeStrategy) -> Option<&'static str> {
    match strategy {
        InvokeStrategy::Cli => Some("cli"),
        InvokeStrategy::Sdk => Some("native"),
        InvokeStrategy::Default | InvokeStrategy::Custom(_) => None,
    }
}
