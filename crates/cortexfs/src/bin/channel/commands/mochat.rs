use std::error::Error;

use super::{super::config::CommonConfig, common};
use cortexfs::channel::mochat::{self, MochatConfig};

pub(super) fn run(common: CommonConfig, config: &MochatConfig) -> Result<(), Box<dyn Error>> {
    let bridge = common::bridge(common)?;
    mochat::run(config, &bridge)?;
    Ok(())
}
