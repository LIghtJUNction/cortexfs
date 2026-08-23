use std::path::{Path, PathBuf};

use crate::channel::adapterstrategy::AdapterStrategy;
use crate::cli::nofollow::open_executable_no_follow;
use crate::support::plain::read_small_text_file;

const MAX_ADAPTER_BYTES: u64 = 256;

/// Reads `channel/<name>.d/adapter`, falling back to the channel id family.
#[must_use]
pub fn read_adapter_strategy(control_dir: &Path, channel: &str) -> AdapterStrategy {
    let strategy = match read_small_text_file(&control_dir.join("adapter"), MAX_ADAPTER_BYTES) {
        Ok(content) => AdapterStrategy::parse(&content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_error) => return AdapterStrategy::Custom(channel.to_owned()),
    };
    strategy.unwrap_or_else(|| {
        AdapterStrategy::family_from_channel_id(channel).map_or_else(
            || AdapterStrategy::Custom(channel.to_owned()),
            AdapterStrategy::Catalog,
        )
    })
}

/// Returns a custom adapter executable when `adapter.d/<name>` is present.
#[must_use]
pub fn resolve_channel_adapter_executable(
    control_dir: &Path,
    strategy: &AdapterStrategy,
) -> Option<PathBuf> {
    let AdapterStrategy::Custom(ref name) = *strategy else {
        return None;
    };
    let custom = control_dir.join("adapter.d").join(name);
    open_executable_no_follow(&custom).ok().map(|_| custom)
}
