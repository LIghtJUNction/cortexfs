#![expect(
    clippy::redundant_pub_crate,
    reason = "wake engine adapter is private driver plumbing"
)]

use std::process::Command;

use serde_json::{Value, json};

use crate::{
    config::Config,
    error::{Error, Result},
};

pub(crate) async fn run(config: &Config, name: &str, _payload: &Value) -> Result<Value> {
    let action = match name {
        "voice_wake.wake" => "wake",
        "voice_wake.stop" => "stop",
        _ => return Err(error("unsupported operation")),
    };
    let executable = config
        .wake_executable
        .clone()
        .ok_or_else(|| error("CORTEXFS_VOICE_WAKE_EXECUTABLE is missing"))?;
    let action = action.to_owned();
    let status = tokio::task::spawn_blocking(move || {
        Command::new(executable)
            .arg(action)
            .status()
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error_message(&error.to_string()))?
    .map_err(|error| error_message(&error))?;
    if !status.success() {
        return Err(error("wake engine rejected operation"));
    }
    Ok(json!({"accepted":true}))
}

fn error(message: &str) -> Error {
    Error::Protocol(message.to_owned())
}

fn error_message(message: &str) -> Error {
    Error::Protocol(format!("wake engine failed: {message}"))
}

#[cfg(test)]
mod tests;
