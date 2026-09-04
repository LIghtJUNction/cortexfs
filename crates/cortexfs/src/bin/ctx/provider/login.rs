use std::io::{self, IsTerminal};
use std::path::Path;
use std::process::ExitCode;

use super::catalog::PROVIDER_PRESETS;
use super::prompt::{ask_api_key, prompt_login_choice};
use super::{
    PROVIDER_CONFIG_DIR, provider_api_key_login, provider_oauth_login, provider_preset_install,
    read_provider_configs_from_dir,
};
use crate::{CliError, print_line};

const DEFAULT_PROFILE: &str = "default";
const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_LOGIN_OPTIONS: usize = 64;

#[derive(Debug)]
pub(crate) struct LoginOption {
    pub(crate) provider: String,
    pub(crate) method: cortexfs::ProviderAuthConfig,
    pub(super) preset: Option<&'static str>,
}

pub(crate) fn provider_login_select() -> Result<ExitCode, CliError> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(CliError::usage(
            "auth login requires a provider when no interactive terminal is available",
        ));
    }
    let options = login_options_from_dir(Path::new(PROVIDER_CONFIG_DIR))?;
    let selected = prompt_login_choice(io::stdin().lock(), &mut io::stdout().lock(), &options)?;
    let Some(option) = selected.and_then(|index| options.get(index)) else {
        print_line("auth login cancelled")?;
        return Ok(ExitCode::SUCCESS);
    };
    match option.method.method {
        cortexfs::AuthMethod::ApiKey => {
            let Some(key) = ask_api_key(&option.provider)? else {
                print_line("auth login cancelled")?;
                return Ok(ExitCode::SUCCESS);
            };
            install_preset(option)?;
            provider_api_key_login(&option.provider, DEFAULT_PROFILE, key.as_bytes())?;
        }
        cortexfs::AuthMethod::OAuth => {
            install_preset(option)?;
            provider_oauth_login(
                &option.provider,
                DEFAULT_PROFILE,
                DEFAULT_TIMEOUT_SECS,
                option.method.flow == Some(cortexfs::OAuthFlow::DeviceCode),
            )?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn install_preset(option: &LoginOption) -> Result<(), CliError> {
    option.preset.map_or(Ok(()), provider_preset_install)
}

pub(crate) fn login_options_from_dir(dir: &Path) -> Result<Vec<LoginOption>, CliError> {
    let mut configs = read_provider_configs_from_dir(dir)?;
    let mut options = Vec::new();
    for preset in PROVIDER_PRESETS {
        if let Some(index) = configs.iter().position(|entry| entry.0 == preset.name) {
            let (provider, config) = configs.remove(index);
            for method in config.auth_methods()? {
                options.push(LoginOption {
                    provider: provider.clone(),
                    method,
                    preset: None,
                });
            }
        } else {
            let method = match preset.auth {
                "api_key" => cortexfs::ProviderAuthConfig::api_key("default"),
                "oauth" => cortexfs::ProviderAuthConfig::oauth(
                    cortexfs::OAuthFlow::AuthorizationCode,
                    "subscription",
                ),
                _ => {
                    return Err(CliError::unavailable(
                        "invalid built-in provider auth method",
                    ));
                }
            };
            options.push(LoginOption {
                provider: preset.name.to_owned(),
                method,
                preset: Some(preset.name),
            });
        }
    }
    for (provider, config) in configs {
        for method in config.auth_methods()? {
            options.push(LoginOption {
                provider: provider.clone(),
                method,
                preset: None,
            });
        }
    }
    if options.len() > MAX_LOGIN_OPTIONS {
        return Err(CliError::unavailable("too many provider login options"));
    }
    Ok(options)
}
