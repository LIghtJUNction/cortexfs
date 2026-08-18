use std::error::Error;

use super::{super::config::CommonConfig, common};
use cortexfs::channel::email::{self, EmailConfig};

pub(super) fn run(common: CommonConfig, config: &EmailConfig) -> Result<(), Box<dyn Error>> {
    let bridge = common::bridge(common)?;
    email::run(config, &bridge)?;
    Ok(())
}
