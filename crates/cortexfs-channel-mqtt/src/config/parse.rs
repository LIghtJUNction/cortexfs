use std::env;

use rumqttc::QoS;

use crate::error::{Error, Result};

pub(super) fn required(name: &'static str) -> Result<String> {
    optional(name).ok_or_else(|| Error::Config(format!("{name} is required")))
}

pub(super) fn optional(name: &'static str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

pub(super) fn list(name: &'static str) -> Result<Vec<String>> {
    let values = required(name)?
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Err(Error::Config(format!("{name} is empty")));
    }
    Ok(values)
}

pub(super) fn number(name: &'static str, default: u64) -> Result<u64> {
    optional(name)
        .unwrap_or_else(|| default.to_string())
        .parse::<u64>()
        .map(|value| value.clamp(5, 300))
        .map_err(|_error| Error::Config(format!("{name} is invalid")))
}

pub(super) fn qos(value: &str) -> Result<QoS> {
    match value {
        "0" => Ok(QoS::AtMostOnce),
        "1" => Ok(QoS::AtLeastOnce),
        "2" => Ok(QoS::ExactlyOnce),
        _ => Err(Error::Config(
            "CORTEXFS_MQTT_QOS must be 0, 1, or 2".to_owned(),
        )),
    }
}
