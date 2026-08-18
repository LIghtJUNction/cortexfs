#![forbid(unsafe_code)]

mod api;
mod config;
mod error;
mod relay;
mod socket;

use std::io::Write as _;

#[tokio::main]
async fn main() {
    let result = match config::Config::load() {
        Ok(config) => relay::run(config).await,
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        let _ignored = writeln!(std::io::stderr(), "cortexfs-channel-slack: {error}");
        std::process::exit(1);
    }
}
