use crate::*;

#[derive(Debug)]
pub(crate) enum ProviderArgs {
    AuthMethods {
        provider: String,
    },
    Login {
        provider: String,
        profile: String,
        timeout: u64,
        device: bool,
    },
    ApiKeyLogin {
        provider: String,
        profile: String,
    },
    Status {
        provider: String,
        profile: String,
    },
    Refresh {
        provider: String,
        profile: String,
    },
    SecretSet {
        provider: String,
        slot: String,
    },
    SecretStatus {
        provider: String,
        slot: String,
    },
    PresetList,
    PresetShow {
        preset: String,
    },
    PresetInstall {
        preset: String,
    },
}

pub(crate) fn parse_provider_command(args: Vec<String>) -> Result<Command, CliError> {
    let mut values = args.into_iter();
    let command = required_arg(
        &mut values,
        "provider requires auth, oauth, preset, or secret",
    )?;
    let rest = values.collect::<Vec<_>>();
    if is_help_args(&rest) {
        return Ok(Command::HelpTopic(format!("provider {command}")));
    }
    if command == "oauth"
        && matches!(rest.as_slice(), [subcommand, help] if is_help_flag(help) && matches!(subcommand.as_str(), "login" | "status" | "refresh"))
    {
        let Some(subcommand) = rest.first() else {
            return Err(CliError::usage(
                "provider oauth requires login, status, or refresh",
            ));
        };
        return Ok(Command::HelpTopic(format!("provider oauth {subcommand}")));
    }
    if command == "auth"
        && matches!(rest.as_slice(), [subcommand, help] if is_help_flag(help) && subcommand == "methods")
    {
        return Ok(Command::HelpTopic("provider auth methods".to_owned()));
    }
    if command == "secret"
        && matches!(rest.as_slice(), [subcommand, help] if is_help_flag(help) && matches!(subcommand.as_str(), "set" | "status"))
    {
        return Ok(Command::HelpTopic("provider secret".to_owned()));
    }
    if command == "help" && rest.is_empty() {
        return Ok(Command::HelpTopic("provider".to_owned()));
    }
    match command.as_str() {
        "oauth" => parse_provider_oauth_command(rest.into_iter()),
        "preset" => parse_provider_preset_command(rest.into_iter()),
        "secret" => parse_provider_secret_command(rest.into_iter()),
        "auth" => parse_provider_auth_command(rest.into_iter()),
        _ => Err(CliError::usage(
            "provider expects auth, oauth, preset, or secret",
        )),
    }
}

/// Parses the unified provider-neutral authorization entrypoint.
pub(crate) fn parse_auth_command(args: Vec<String>) -> Result<Command, CliError> {
    let mut values = args.into_iter();
    let command = required_arg(
        &mut values,
        "auth requires methods, login, status, or refresh",
    )?;
    let rest = values.collect::<Vec<_>>();
    if is_help_args(&rest) {
        return Ok(Command::HelpTopic("auth".to_owned()));
    }
    let provider = match command.as_str() {
        "methods" => parse_provider_auth_command(
            std::iter::once(command)
                .chain(rest)
                .collect::<Vec<_>>()
                .into_iter(),
        )?,
        "login" => parse_auth_login(rest.into_iter())?,
        "status" | "refresh" => parse_provider_oauth_command(
            std::iter::once(command)
                .chain(rest)
                .collect::<Vec<_>>()
                .into_iter(),
        )?,
        _ => {
            return Err(CliError::usage(
                "auth expects methods, login, status, or refresh",
            ));
        }
    };
    let Command::Provider(provider) = provider else {
        return Err(CliError::usage("invalid auth command"));
    };
    Ok(Command::Auth(provider))
}

fn parse_auth_login(mut values: impl Iterator<Item = String>) -> Result<Command, CliError> {
    let provider = required_arg(&mut values, "auth login requires a provider")?;
    let mut profile = "default".to_owned();
    let mut timeout = 120;
    let mut method = "auto".to_owned();
    let mut device = false;
    let mut stdin = false;
    while let Some(value) = values.next() {
        match value.as_str() {
            "--profile" => {
                profile = required_arg(&mut values, "auth login --profile requires a name")?;
            }
            "--method" => {
                method = required_arg(&mut values, "auth login --method requires a value")?;
            }
            "--timeout" => {
                timeout = required_arg(&mut values, "auth login --timeout requires seconds")?
                    .parse::<u64>()
                    .map_err(|_error| CliError::usage("invalid auth timeout"))?;
            }
            "--device" => device = true,
            "--stdin" => stdin = true,
            _ => return Err(CliError::usage(format!("unexpected argument: {value}"))),
        }
    }
    if !is_provider_name(&provider) || !is_provider_secret_slot(&profile) {
        return Err(CliError::usage("invalid authentication profile"));
    }
    if method == "api-key" {
        return stdin
            .then_some(Command::Provider(ProviderArgs::ApiKeyLogin {
                provider,
                profile,
            }))
            .ok_or_else(|| CliError::usage("auth login --method api-key requires --stdin"));
    }
    let device = match method.as_str() {
        "auto" | "browser" if !device => false,
        "auto" | "device" => true,
        _ => return Err(CliError::usage("auth login method is unsupported")),
    };
    Ok(Command::Provider(ProviderArgs::Login {
        provider,
        profile,
        timeout,
        device,
    }))
}

pub(crate) fn parse_provider_auth_command(
    mut values: impl Iterator<Item = String>,
) -> Result<Command, CliError> {
    let command = required_arg(&mut values, "provider auth requires methods")?;
    if command != "methods" {
        return Err(CliError::usage("provider auth expects methods"));
    }
    let provider = required_arg(&mut values, "provider auth methods requires a provider")?;
    no_extra_args(values)?;
    Ok(Command::Provider(ProviderArgs::AuthMethods { provider }))
}

pub(crate) fn parse_provider_oauth_command(
    mut values: impl Iterator<Item = String>,
) -> Result<Command, CliError> {
    let command = required_arg(
        &mut values,
        "provider oauth requires login, status, or refresh",
    )?;
    match command.as_str() {
        "login" => {
            let provider = required_arg(&mut values, "provider oauth login requires a provider")?;
            let mut timeout = 120;
            let mut device = false;
            let mut profile = "default".to_owned();
            while let Some(value) = values.next() {
                match value.as_str() {
                    "--device" => device = true,
                    "--profile" => {
                        profile = required_arg(
                            &mut values,
                            "provider oauth login --profile requires a name",
                        )?;
                    }
                    "--timeout" => {
                        let raw = required_arg(
                            &mut values,
                            "provider oauth login --timeout requires seconds",
                        )?;
                        timeout = raw
                            .parse::<u64>()
                            .map_err(|_error| CliError::usage("invalid oauth timeout"))?;
                    }
                    _ => return Err(CliError::usage(format!("unexpected argument: {value}"))),
                }
            }
            if !is_provider_secret_slot(&profile) {
                return Err(CliError::usage("invalid authentication profile"));
            }
            Ok(Command::Provider(ProviderArgs::Login {
                provider,
                profile,
                timeout,
                device,
            }))
        }
        "status" => {
            let provider = required_arg(&mut values, "provider oauth status requires a provider")?;
            let profile = parse_provider_profile(values)?;
            Ok(Command::Provider(ProviderArgs::Status {
                provider,
                profile,
            }))
        }
        "refresh" => {
            let provider = required_arg(&mut values, "provider oauth refresh requires a provider")?;
            let profile = parse_provider_profile(values)?;
            Ok(Command::Provider(ProviderArgs::Refresh {
                provider,
                profile,
            }))
        }
        _ => Err(CliError::usage(
            "provider oauth expects login, status, or refresh",
        )),
    }
}

fn parse_provider_profile(mut values: impl Iterator<Item = String>) -> Result<String, CliError> {
    let Some(flag) = values.next() else {
        return Ok("default".to_owned());
    };
    if flag != "--profile" {
        return Err(CliError::usage(format!("unexpected argument: {flag}")));
    }
    let profile = required_arg(&mut values, "provider oauth --profile requires a name")?;
    no_extra_args(values)?;
    is_provider_secret_slot(&profile)
        .then_some(profile)
        .ok_or_else(|| CliError::usage("invalid authentication profile"))
}

pub(crate) fn parse_provider_preset_command(
    mut values: impl Iterator<Item = String>,
) -> Result<Command, CliError> {
    let command = required_arg(
        &mut values,
        "provider preset requires list, show, or install",
    )?;
    match command.as_str() {
        "list" => {
            no_extra_args(values)?;
            Ok(Command::Provider(ProviderArgs::PresetList))
        }
        "show" => {
            let preset = required_arg(&mut values, "provider preset show requires a preset")?;
            no_extra_args(values)?;
            Ok(Command::Provider(ProviderArgs::PresetShow { preset }))
        }
        "install" => {
            let preset = required_arg(&mut values, "provider preset install requires a preset")?;
            no_extra_args(values)?;
            Ok(Command::Provider(ProviderArgs::PresetInstall { preset }))
        }
        _ => Err(CliError::usage(
            "provider preset expects list, show, or install",
        )),
    }
}

pub(crate) fn parse_provider_secret_command(
    mut values: impl Iterator<Item = String>,
) -> Result<Command, CliError> {
    let command = required_arg(&mut values, "provider secret requires set or status")?;
    match command.as_str() {
        "set" => {
            let provider = required_arg(&mut values, "provider secret set requires a provider")?;
            let slot = parse_provider_secret_slot(values)?;
            Ok(Command::Provider(ProviderArgs::SecretSet {
                provider,
                slot,
            }))
        }
        "status" => {
            let provider = required_arg(&mut values, "provider secret status requires a provider")?;
            let slot = parse_provider_secret_slot(values)?;
            Ok(Command::Provider(ProviderArgs::SecretStatus {
                provider,
                slot,
            }))
        }
        _ => Err(CliError::usage("provider secret expects set or status")),
    }
}

pub(crate) fn parse_provider_secret_slot(
    mut values: impl Iterator<Item = String>,
) -> Result<String, CliError> {
    let mut slot = "default".to_owned();
    while let Some(value) = values.next() {
        match value.as_str() {
            "--slot" => {
                slot = required_arg(&mut values, "provider secret --slot requires a value")?;
            }
            _ => return Err(CliError::usage(format!("unexpected argument: {value}"))),
        }
    }
    Ok(slot)
}

pub(crate) fn is_help_flag(value: &str) -> bool {
    matches!(value, "--help" | "-h")
}

pub(crate) fn parse_ls_command(
    mut values: impl Iterator<Item = String>,
) -> Result<Command, CliError> {
    let target = values.next().map_or(LsTarget::Root, LsTarget::Path);
    no_extra_args(values)?;
    Ok(Command::Ls(target))
}

pub(crate) fn parse_mount_command(
    mut values: impl Iterator<Item = String>,
) -> Result<Command, CliError> {
    let mut source = None;
    let mut mountpoint = None;

    while let Some(value) = values.next() {
        match value.as_str() {
            "--source" | "-s" => {
                let next = required_arg(&mut values, "mount --source requires a path")?;
                source = Some(PathBuf::from(next));
            }
            _ => {
                if mountpoint.is_some() {
                    return Err(CliError::usage(format!("unexpected argument: {value}")));
                }
                mountpoint = Some(PathBuf::from(value));
            }
        }
    }

    Ok(Command::Mount { source, mountpoint })
}
