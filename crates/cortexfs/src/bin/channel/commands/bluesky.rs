use std::error::Error;

use super::{super::config::CommonConfig, common};
use cortexfs::channel::bluesky::{self, BlueskyConfig};

pub(super) fn run(
    common: CommonConfig,
    handle: String,
    app_password: String,
    api_base: String,
    poll: u64,
) -> Result<(), Box<dyn Error>> {
    let bridge = common::bridge(common)?;
    let config = BlueskyConfig::new(handle, app_password, api_base)?.with_poll_seconds(poll);
    bluesky::run(&config, &bridge)?;
    Ok(())
}
