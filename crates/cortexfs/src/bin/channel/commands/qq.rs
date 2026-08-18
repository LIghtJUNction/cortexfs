use std::error::Error;

use super::{super::config::CommonConfig, common};
use cortexfs::channel::qq::{self, QqConfig};

pub(super) fn run(common: CommonConfig, config: &QqConfig) -> Result<(), Box<dyn Error>> {
    let bridge = common::bridge(common)?;
    qq::run(config, &bridge)?;
    Ok(())
}
