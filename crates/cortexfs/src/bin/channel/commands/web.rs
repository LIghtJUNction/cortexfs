use std::{error::Error, net::SocketAddr};

use super::super::{
    config::CommonConfig,
    web::{self, WebConfig},
};

pub(super) fn run(
    common: CommonConfig,
    bind: SocketAddr,
    path: String,
    token: Option<String>,
) -> Result<(), Box<dyn Error>> {
    web::run(&WebConfig {
        socket: common.socket,
        bind,
        path,
        token,
    })?;
    Ok(())
}
