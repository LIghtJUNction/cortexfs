use std::error::Error;

use super::{super::config::CommonConfig, common};
use cortexfs::channel::reddit::{self, RedditConfig};

pub(super) fn run(common: CommonConfig, config: &RedditConfig) -> Result<(), Box<dyn Error>> {
    let bridge = common::bridge(common)?;
    reddit::run(config, &bridge)?;
    Ok(())
}
