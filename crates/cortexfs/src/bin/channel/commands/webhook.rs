use std::error::Error;

use super::super::webhook::{self, WebhookConfig};
use super::{super::config::CommonConfig, common};

pub(super) fn run(common: CommonConfig, config: &WebhookConfig) -> Result<(), Box<dyn Error>> {
    let bridge = common::bridge(common)?;
    webhook::run(config, &bridge)?;
    Ok(())
}
