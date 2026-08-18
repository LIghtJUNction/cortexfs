use std::error::Error;

use super::{super::config::CommonConfig, common};
use cortexfs::channel::twitter::{self, TwitterConfig};

pub(super) fn run(common: CommonConfig, config: &TwitterConfig) -> Result<(), Box<dyn Error>> {
    let bridge = common::bridge(common)?;
    twitter::run(config, &bridge)?;
    Ok(())
}
