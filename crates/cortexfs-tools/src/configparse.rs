use crate::{MAX_TSH_TOOL_COUNT, TshRuntimeConfig};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TshConfigParseError(String);

impl Display for TshConfigParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for TshConfigParseError {}

pub fn parse_tsh_runtime_config(content: &str) -> Result<TshRuntimeConfig, TshConfigParseError> {
    let mut config = TshRuntimeConfig::default();
    for (index, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(parse_error(index, "must be key=value"));
        };
        let value = value
            .parse::<usize>()
            .map_err(|_error| parse_error(index, "value must be a positive integer"))?;
        match key {
            "max_loaded_tools" | "cache_capacity" if (1..=MAX_TSH_TOOL_COUNT).contains(&value) => {
                if key == "max_loaded_tools" {
                    config.max_loaded_tools = value;
                } else {
                    config.cache_capacity = value;
                }
            }
            "window_percent" if (1..=100).contains(&value) => config.window_percent = value,
            "max_loaded_tools" | "cache_capacity" => {
                return Err(parse_error(
                    index,
                    &format!("value must be 1..{MAX_TSH_TOOL_COUNT}"),
                ));
            }
            "window_percent" => return Err(parse_error(index, "window_percent must be 1..100")),
            _ => return Err(parse_error(index, &format!("has unknown key {key}"))),
        }
    }
    Ok(config)
}

fn parse_error(index: usize, message: &str) -> TshConfigParseError {
    TshConfigParseError(format!("line {} {message}", index.saturating_add(1)))
}
