use std::error::Error;

use super::{super::config::CommonConfig, common};
use cortexfs::channel::matrix::{self, MatrixConfig};

pub(super) fn run(
    common: CommonConfig,
    homeserver: String,
    token: String,
    rooms: Vec<String>,
    sync: u64,
) -> Result<(), Box<dyn Error>> {
    let bridge = common::bridge(common)?;
    let config = MatrixConfig::new(homeserver, token, rooms)?.with_sync_seconds(sync);
    matrix::run(&config, &bridge)?;
    Ok(())
}
