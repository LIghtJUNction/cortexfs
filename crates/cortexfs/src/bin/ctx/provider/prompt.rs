use std::io::{BufRead, Write};
use std::process::{Command, Stdio};

use super::login::LoginOption;
use crate::{CliError, MAX_PROVIDER_SECRET_STDIN_BYTES};

impl LoginOption {
    fn label(&self) -> String {
        let method = match (self.method.method, self.method.flow) {
            (cortexfs::AuthMethod::ApiKey, _) => "API key",
            (cortexfs::AuthMethod::OAuth, Some(cortexfs::OAuthFlow::DeviceCode)) => {
                "OAuth (device code)"
            }
            (cortexfs::AuthMethod::OAuth, _) => "OAuth (browser)",
        };
        let state = if self.preset.is_some() {
            "installs provider preset"
        } else {
            "provider configured"
        };
        format!("{}  {method}  [{state}]", self.provider)
    }
}

pub(crate) fn prompt_login_choice(
    mut input: impl BufRead,
    output: &mut impl Write,
    options: &[LoginOption],
) -> Result<Option<usize>, CliError> {
    writeln!(output, "Select a provider login:")
        .and_then(|()| {
            options.iter().enumerate().try_for_each(|(index, option)| {
                writeln!(output, "  {:>2}) {}", index + 1, option.label())
            })
        })
        .and_then(|()| write!(output, "   q) Cancel\n› "))
        .and_then(|()| output.flush())
        .map_err(|error| CliError::unavailable(format!("terminal write failed: {error}")))?;
    let mut choice = String::new();
    input
        .read_line(&mut choice)
        .map_err(|error| CliError::unavailable(format!("terminal read failed: {error}")))?;
    let choice = choice.trim();
    if choice.is_empty() || matches!(choice, "q" | "quit") {
        return Ok(None);
    }
    choice
        .parse::<usize>()
        .ok()
        .filter(|value| (1..=options.len()).contains(value))
        .map(|value| Some(value - 1))
        .ok_or_else(|| {
            CliError::usage(format!("choose a login option from 1 to {}", options.len()))
        })
}

pub(super) fn ask_api_key(provider: &str) -> Result<Option<String>, CliError> {
    let output = Command::new("/usr/bin/systemd-ask-password")
        .args(["--timeout=0", "-n"])
        .arg(format!("{provider} API key:"))
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| CliError::unavailable(format!("cannot prompt for API key: {error}")))?;
    if !output.status.success() {
        return Ok(None);
    }
    if output.stdout.is_empty() || output.stdout.len() > MAX_PROVIDER_SECRET_STDIN_BYTES {
        return Err(CliError::usage(
            "auth login requires a non-empty bounded API key",
        ));
    }
    String::from_utf8(output.stdout)
        .map(Some)
        .map_err(|_error| CliError::usage("API key is not utf-8"))
}
