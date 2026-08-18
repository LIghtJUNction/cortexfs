use std::{error::Error, net::SocketAddr};

use super::super::gmail::{self, GmailConfig};
use super::{super::config::CommonConfig, common};

pub(super) fn run(
    common: CommonConfig,
    bind: SocketAddr,
    path: String,
    access: String,
    base: String,
    token: Option<String>,
) -> Result<(), Box<dyn Error>> {
    let bridge = common::bridge(common)?;
    let config = GmailConfig {
        socket: bridge.socket().to_owned(),
        bind,
        path,
        access_token: access,
        api_base: base,
        token,
    };
    gmail::run(&config, &bridge)?;
    Ok(())
}
