use std::error::Error;

use super::{super::config::CommonConfig, common};
use cortexfs::channel::signal::{self, SignalConfig};

pub(super) fn run(
    common: CommonConfig,
    account: String,
    executable: String,
) -> Result<(), Box<dyn Error>> {
    let bridge = common::bridge(common)?;
    let config = SignalConfig::new(account, executable)?;
    signal::run(&config, &bridge)?;
    Ok(())
}
