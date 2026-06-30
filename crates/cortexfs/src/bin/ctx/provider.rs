include!("provider/secrets.rs");
include!("provider/presets.rs");
include!("provider/config_files.rs");
include!("provider/oauth.rs");
include!("provider/oauth_callback.rs");

fn provider_command(args: &ProviderArgs) -> Result<ExitCode, CliError> {
    match *args {
        ProviderArgs::Login {
            ref provider,
            timeout,
        } => provider_oauth_login(provider, timeout).map(|()| ExitCode::SUCCESS),
        ProviderArgs::Status { ref provider } => {
            provider_oauth_status(provider).map(|()| ExitCode::SUCCESS)
        }
        ProviderArgs::Refresh { ref provider } => {
            provider_oauth_refresh(provider).map(|()| ExitCode::SUCCESS)
        }
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
