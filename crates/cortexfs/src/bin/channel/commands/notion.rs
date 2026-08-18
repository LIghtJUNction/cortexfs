use std::error::Error;

use super::{super::config::CommonConfig, common};
use cortexfs::channel::notion::{self, NotionConfig};

pub(super) fn run(common: CommonConfig, config: &NotionConfig) -> Result<(), Box<dyn Error>> {
    let bridge = common::bridge(common)?;
    notion::run(config, &bridge)?;
    Ok(())
}
