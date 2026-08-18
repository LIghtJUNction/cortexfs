use std::error::Error;

use super::{super::config::CommonConfig, common};
use cortexfs::channel::twitch::{self, TwitchConfig};

pub(super) fn run(common: CommonConfig, config: &TwitchConfig) -> Result<(), Box<dyn Error>> {
    let bridge = common::bridge(common)?;
    twitch::run(config, &bridge)?;
    Ok(())
}
