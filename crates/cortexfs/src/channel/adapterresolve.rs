use std::path::{Path, PathBuf};

use crate::channel::adapterstrategy::AdapterStrategy;
use crate::cli::nofollow::open_executable_no_follow;
use crate::support::plain::read_small_text_file;

const MAX_ADAPTER_BYTES: u64 = 256;

fn adapter_strategy_from_channel(channel: &str) -> AdapterStrategy {
    AdapterStrategy::family_from_channel_id(channel).map_or_else(
        || AdapterStrategy::Custom(channel.to_owned()),
        AdapterStrategy::Catalog,
    )
}

/// Reads `channel/<name>.d/adapter`, falling back to the channel id family.
#[must_use]
pub fn read_adapter_strategy(control_dir: &Path, channel: &str) -> AdapterStrategy {
    match read_small_text_file(&control_dir.join("adapter"), MAX_ADAPTER_BYTES) {
        Ok(content) => AdapterStrategy::parse(&content)
            .unwrap_or_else(|| adapter_strategy_from_channel(channel)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            adapter_strategy_from_channel(channel)
        }
        Err(_error) => AdapterStrategy::Custom(channel.to_owned()),
    }
}

/// Returns a custom adapter executable when `adapter.d/<name>` is present.
#[must_use]
#[expect(
    clippy::pattern_type_mismatch,
    reason = "strategy is matched by reference for caller-owned values"
)]
pub fn resolve_channel_adapter_executable(
    control_dir: &Path,
    strategy: &AdapterStrategy,
) -> Option<PathBuf> {
    match strategy {
        AdapterStrategy::Custom(name) => {
            let custom = control_dir.join("adapter.d").join(name);
            open_executable_no_follow(&custom).ok().map(|_| custom)
        }
        AdapterStrategy::Catalog(_) => None,
    }
}
