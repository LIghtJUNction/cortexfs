use crate::*;

pub(crate) use auth::*;
pub(crate) use callback::*;
pub(crate) use config::*;
#[cfg(test)]
pub(crate) use login::login_options_from_dir;
pub(crate) use login::provider_login_select;
pub(crate) use oauth::*;
pub(crate) use presets::*;
#[cfg(test)]
pub(crate) use prompt::prompt_login_choice;
pub(crate) use secrets::*;

pub mod auth;
pub mod callback;
pub mod catalog;
pub mod config;
mod login;
pub mod oauth;
pub mod presets;
mod prompt;
pub mod secrets;

pub(crate) fn provider_command(args: &ProviderArgs) -> Result<ExitCode, CliError> {
    match *args {
        ProviderArgs::AuthMethods { ref provider } => {
            provider_auth_methods(provider).map(|()| ExitCode::SUCCESS)
        }
        ProviderArgs::Login {
            ref provider,
            ref profile,
            timeout,
            device,
        } => provider_oauth_login(provider, profile, timeout, device).map(|()| ExitCode::SUCCESS),
        ProviderArgs::LoginSelect => provider_login_select(),
        ProviderArgs::ApiKeyLogin {
            ref provider,
            ref profile,
        } => provider_api_key_login(provider, profile, io::stdin()).map(|()| ExitCode::SUCCESS),
        ProviderArgs::Logout {
            ref provider,
            ref profile,
        } => provider_auth_logout(provider, profile).map(|()| ExitCode::SUCCESS),
        ProviderArgs::Status {
            ref provider,
            ref profile,
        } => provider_oauth_status(provider, profile).map(|()| ExitCode::SUCCESS),
        ProviderArgs::Refresh {
            ref provider,
            ref profile,
        } => provider_oauth_refresh(provider, profile).map(|()| ExitCode::SUCCESS),
        ProviderArgs::SecretSet {
            ref provider,
            ref slot,
        } => provider_secret_set(provider, slot).map(|()| ExitCode::SUCCESS),
        ProviderArgs::SecretStatus {
            ref provider,
            ref slot,
        } => provider_secret_status(provider, slot).map(|()| ExitCode::SUCCESS),
        ProviderArgs::PresetList => provider_preset_list().map(|()| ExitCode::SUCCESS),
        ProviderArgs::PresetShow { ref preset } => {
            provider_preset_show(preset).map(|()| ExitCode::SUCCESS)
        }
        ProviderArgs::PresetInstall {
            ref preset,
            ref name,
            ref base_url,
            ref model,
        } => if preset == "compatible" {
            provider_preset_install_compatible(
                name.as_deref(),
                base_url.as_deref(),
                model.as_deref(),
            )
        } else if name.is_some() || base_url.is_some() || model.is_some() {
            Err(CliError::usage(
                "compatible flags are only valid for the compatible preset",
            ))
        } else {
            provider_preset_install(preset)
        }
        .map(|()| ExitCode::SUCCESS),
    }
}
