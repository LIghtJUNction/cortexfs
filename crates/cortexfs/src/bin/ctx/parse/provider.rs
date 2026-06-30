#[derive(Debug)]
enum ProviderArgs {
    Login { provider: String, timeout: u64 },
    Status { provider: String },
    Refresh { provider: String },
    SecretSet { provider: String, slot: String },
    SecretStatus { provider: String, slot: String },
    PresetList,
    PresetShow { preset: String },
    PresetInstall { preset: String },
}

fn parse_provider_command(args: Vec<String>) -> Result<Command, CliError> {
    let mut values = args.into_iter();
    let command = required_arg(&mut values, "provider requires oauth, preset, or secret")?;
    let rest = values.collect::<Vec<_>>();
    if is_help_args(&rest) {
        return Ok(Command::HelpTopic(format!("provider {command}")));
    }
    if command == "oauth"
        && matches!(rest.as_slice(), [subcommand, help] if is_help_flag(help) && matches!(subcommand.as_str(), "login" | "status" | "refresh"))
    {
        let Some(subcommand) = rest.first() else {
            return Err(CliError::usage("provider oauth requires login, status, or refresh"));
        };
        return Ok(Command::HelpTopic(format!("provider oauth {subcommand}")));
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
        _ => Err(CliError::usage("provider expects oauth, preset, or secret")),
    }
}

fn parse_provider_oauth_command(mut values: impl Iterator<Item = String>) -> Result<Command, CliError> {
    let command = required_arg(&mut values, "provider oauth requires login, status, or refresh")?;
    match command.as_str() {
        "login" => {
            let provider = required_arg(&mut values, "provider oauth login requires a provider")?;
            let mut timeout = 120;
            while let Some(value) = values.next() {
                match value.as_str() {
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
            Ok(Command::Provider(ProviderArgs::Login { provider, timeout }))
        }
        "status" => {
            let provider = required_arg(&mut values, "provider oauth status requires a provider")?;
            no_extra_args(values)?;
            Ok(Command::Provider(ProviderArgs::Status { provider }))
        }
        "refresh" => {
            let provider = required_arg(&mut values, "provider oauth refresh requires a provider")?;
            no_extra_args(values)?;
            Ok(Command::Provider(ProviderArgs::Refresh { provider }))
        }
        _ => Err(CliError::usage("provider oauth expects login, status, or refresh")),
    }
}

fn parse_provider_preset_command(mut values: impl Iterator<Item = String>) -> Result<Command, CliError> {
    let command = required_arg(&mut values, "provider preset requires list, show, or install")?;
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
        _ => Err(CliError::usage("provider preset expects list, show, or install")),
    }
}

fn parse_provider_secret_command(
    mut values: impl Iterator<Item = String>,
) -> Result<Command, CliError> {
    let command = required_arg(&mut values, "provider secret requires set or status")?;
    match command.as_str() {
        "set" => {
            let provider = required_arg(&mut values, "provider secret set requires a provider")?;
            let slot = parse_provider_secret_slot(values)?;
            Ok(Command::Provider(ProviderArgs::SecretSet { provider, slot }))
        }
        "status" => {
            let provider = required_arg(&mut values, "provider secret status requires a provider")?;
            let slot = parse_provider_secret_slot(values)?;
            Ok(Command::Provider(ProviderArgs::SecretStatus { provider, slot }))
        }
        _ => Err(CliError::usage("provider secret expects set or status")),
    }
}

fn parse_provider_secret_slot(mut values: impl Iterator<Item = String>) -> Result<String, CliError> {
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

fn is_help_flag(value: &str) -> bool {
    matches!(value, "--help" | "-h")
}

fn parse_ls_command(mut values: impl Iterator<Item = String>) -> Result<Command, CliError> {
    let target = values.next().map_or(LsTarget::Root, LsTarget::Path);
    no_extra_args(values)?;
    Ok(Command::Ls(target))
}

fn parse_mount_command(mut values: impl Iterator<Item = String>) -> Result<Command, CliError> {
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
