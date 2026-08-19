use std::error::Error;

use super::{super::config::CommonConfig, common};
use cortexfs::channel::telegram::{self, TelegramConfig};

pub(super) fn run(
    common: CommonConfig,
    token: String,
    api_base: String,
    poll: u64,
) -> Result<(), Box<dyn Error>> {
    let progress = common.progress.clone();
    let bridge = common::bridge(common)?;
    let config = TelegramConfig::new(token, api_base)?
        .with_poll_seconds(poll)
        .with_progress(progress);
    telegram::run(&config, &bridge)?;
    Ok(())
}
