use std::error::Error;

use super::{super::config::CommonConfig, common};
use cortexfs::channel::mattermost::{self, MattermostConfig};

pub(super) fn run(
    common: CommonConfig,
    base_url: String,
    token: String,
    channels: Vec<String>,
    reconnect_seconds: u64,
) -> Result<(), Box<dyn Error>> {
    let bridge = common::bridge(common)?;
    let config =
        MattermostConfig::new(base_url, token, channels)?.with_reconnect_seconds(reconnect_seconds);
    mattermost::run(&config, &bridge)?;
    Ok(())
}
