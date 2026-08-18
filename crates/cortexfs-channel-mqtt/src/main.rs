#![forbid(unsafe_code)]

mod config;
mod error;
mod message;
mod relay;
mod socket;

#[cfg(test)]
mod tests;

use error::Result;
use std::io::Write as _;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        let _ignored = writeln!(std::io::stderr(), "cortexfs-channel-mqtt: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    relay::run(config::Config::load()?).await
}
