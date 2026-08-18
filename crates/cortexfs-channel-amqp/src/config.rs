#![expect(
    clippy::field_scoped_visibility_modifiers,
    clippy::redundant_pub_crate,
    reason = "configuration is shared by private driver modules"
)]

use std::{env, path::PathBuf};

use crate::error::{Error, Result};

pub(crate) struct Config {
    pub(crate) url: String,
    pub(crate) exchange: String,
    pub(crate) queue: String,
    pub(crate) routing_keys: Vec<String>,
    pub(crate) prefetch: u16,
    pub(crate) durable_ack: bool,
    pub(crate) socket: PathBuf,
}

impl Config {
    pub(crate) fn load() -> Result<Self> {
        let expected = cortexfs_paths::channel_driver_socket("amqp");
        let socket = PathBuf::from(
            env::var("CORTEXFS_CHANNEL_SOCKET").unwrap_or_else(|_| expected.display().to_string()),
        );
        if socket != expected {
            return Err(Error::Config(format!(
                "CORTEXFS_CHANNEL_SOCKET must be {}",
                expected.display()
            )));
        }
        Ok(Self {
            url: required("CORTEXFS_AMQP_URL")?,
            exchange: required("CORTEXFS_AMQP_EXCHANGE")?,
            queue: required("CORTEXFS_AMQP_QUEUE")?,
            routing_keys: list("CORTEXFS_AMQP_ROUTING_KEYS")?,
            prefetch: number("CORTEXFS_AMQP_PREFETCH", 4)?.clamp(1, 16),
            durable_ack: boolean("CORTEXFS_AMQP_DURABLE_ACK", true)?,
            socket,
        })
    }
}

fn required(name: &'static str) -> Result<String> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Error::Config(format!("{name} is required")))
}

fn list(name: &'static str) -> Result<Vec<String>> {
    let values = parse_list(&required(name)?);
    if values.is_empty() {
        return Err(Error::Config(format!("{name} is empty")));
    }
    Ok(values)
}

fn number(name: &'static str, default: u16) -> Result<u16> {
    parse_number(
        name,
        &env::var(name).unwrap_or_else(|_| default.to_string()),
    )
}

fn boolean(name: &'static str, default: bool) -> Result<bool> {
    parse_boolean(
        name,
        &env::var(name).unwrap_or_else(|_| default.to_string()),
    )
}

fn parse_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_number(name: &'static str, value: &str) -> Result<u16> {
    value
        .parse()
        .map_err(|_error| Error::Config(format!("{name} is invalid")))
}

fn parse_boolean(name: &'static str, value: &str) -> Result<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(Error::Config(format!("{name} is invalid"))),
    }
}

#[cfg(test)]
mod tests;
