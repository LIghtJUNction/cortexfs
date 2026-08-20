use crate::*;

pub(crate) use auth::*;
pub(crate) use callback::*;
pub(crate) use config::*;
pub(crate) use oauth::*;
pub(crate) use presets::*;
pub(crate) use secrets::*;

pub mod auth;
pub mod callback;
pub mod config;
pub mod oauth;
pub mod presets;
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
        ProviderArgs::ApiKeyLogin {
            ref provider,
            ref profile,
        } => provider_api_key_login(provider, profile).map(|()| ExitCode::SUCCESS),
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
        ProviderArgs::PresetInstall { ref preset } => {
            provider_preset_install(preset).map(|()| ExitCode::SUCCESS)
        }
    }
}
