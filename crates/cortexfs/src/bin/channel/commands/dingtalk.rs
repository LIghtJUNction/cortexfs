use std::error::Error;

use super::{super::config::CommonConfig, common};
use cortexfs::channel::dingtalk::{self, DingTalkConfig};

pub(super) fn run(
    common: CommonConfig,
    id: String,
    secret: String,
    gateway: String,
) -> Result<(), Box<dyn Error>> {
    let bridge = common::bridge(common)?;
    let config = DingTalkConfig::new(id, secret, gateway)?;
    dingtalk::run(&config, &bridge)?;
    Ok(())
}
