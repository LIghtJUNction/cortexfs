use std::{error::Error, path::PathBuf};

use super::{super::config::CommonConfig, common};
use cortexfs::channel::driver::{self, DriverConfig, DriverHub};
use cortexfs_channels::ChannelId;

pub(super) fn run(
    common: CommonConfig,
    channel: ChannelId,
    socket: PathBuf,
) -> Result<(), Box<dyn Error>> {
    let bridge = common::bridge(common)?;
    driver::run(&DriverConfig {
        socket,
        channel,
        bridge,
        hub: DriverHub::default(),
    })?;
    Ok(())
}
