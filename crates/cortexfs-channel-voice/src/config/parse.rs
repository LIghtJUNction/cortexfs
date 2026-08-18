use std::{collections::BTreeSet, env, time::Duration};

use crate::{
    config::Provider,
    error::{Error, Result},
};

pub(super) fn provider(value: Option<&str>) -> Result<Provider> {
    match value.unwrap_or("telnyx") {
        "twilio" => Ok(Provider::Twilio),
        "telnyx" => Ok(Provider::Telnyx),
        "plivo" => Ok(Provider::Plivo),
        value => Err(Error::Config(format!("unknown voice provider: {value}"))),
    }
}

pub(super) fn required(name: &'static str) -> Result<String> {
    optional(name).ok_or_else(|| Error::Config(format!("{name} is required")))
}

pub(super) fn optional(name: &'static str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

pub(super) fn list(name: &'static str) -> BTreeSet<String> {
    env::var(name)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

pub(super) fn seconds(name: &'static str) -> Result<Option<Duration>> {
    let Some(value) = optional(name) else {
        return Ok(None);
    };
    let value = value
        .parse::<u64>()
        .map_err(|_error| Error::Config(format!("{name} is invalid")))?;
    Ok((value > 0).then(|| Duration::from_secs(value.min(600))))
}
