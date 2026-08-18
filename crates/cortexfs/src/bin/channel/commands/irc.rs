use std::error::Error;

use super::{super::config::CommonConfig, common};
use cortexfs::channel::irc::{self, IrcConfig};

pub(super) fn run(
    common: CommonConfig,
    server: String,
    port: u16,
    nickname: String,
    channels: Vec<String>,
    password: Option<String>,
) -> Result<(), Box<dyn Error>> {
    let bridge = common::bridge(common)?;
    let mut config = IrcConfig::new(server, port, nickname, channels)?;
    config.password = password;
    irc::run(&config, &bridge)?;
    Ok(())
}
