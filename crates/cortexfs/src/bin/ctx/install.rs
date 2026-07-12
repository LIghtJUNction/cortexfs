use crate::{CliError, Command, print_line, required_arg, terminal_safe_text};
use cortexfs::object::install::{InstallError, InstallTier};
use std::path::{Path, PathBuf};

pub(crate) fn parse_object_command(
    mut values: impl Iterator<Item = String>,
) -> Result<Command, CliError> {
    let action = required_arg(&mut values, "object requires check, install, or residue")?;
    if action == "check" {
        let manifest = PathBuf::from(required_arg(
            &mut values,
            "object check requires a manifest path",
        )?);
        if manifest.as_os_str().to_string_lossy().starts_with('-') {
            return Err(CliError::usage(format!(
                "unexpected object check argument: {}",
                manifest.display()
            )));
        }
        if let Some(value) = values.next() {
            return Err(CliError::usage(format!(
                "unexpected object check argument: {value}"
            )));
        }
        return Ok(Command::ObjectCheck { manifest });
    }
    if action == "residue" {
        return crate::residue::parse_object_residue_command(values);
    }
    if action != "install" {
        return Err(CliError::usage("object expects check, install, or residue"));
    }
    let mut manifest = None;
    let mut source = None;
    let mut tier = InstallTier::User;
    let mut tier_seen = false;
    while let Some(value) = values.next() {
        match value.as_str() {
            "--source" => {
                if source.is_some() {
                    return Err(CliError::usage("object install --source specified twice"));
                }
                source = Some(PathBuf::from(required_arg(
                    &mut values,
                    "object install --source requires a durable backing path",
                )?));
            }
            "--tier" => {
                if tier_seen {
                    return Err(CliError::usage("object install --tier specified twice"));
                }
                tier_seen = true;
                tier = match required_arg(&mut values, "--tier requires user or system")?.as_str() {
                    "user" => InstallTier::User,
                    "system" => InstallTier::System,
                    _ => return Err(CliError::usage("--tier expects user or system")),
                };
            }
            _ if value.starts_with('-') => {
                return Err(CliError::usage(format!(
                    "unexpected object install argument: {value}"
                )));
            }
            _ if manifest.is_none() => manifest = Some(PathBuf::from(value)),
            _ => return Err(CliError::usage("object install accepts one manifest path")),
        }
    }
    let manifest =
        manifest.ok_or_else(|| CliError::usage("object install requires a manifest path"))?;
    let source = source.ok_or_else(|| CliError::usage(
        "object install requires --source PATH for the durable backing tree; /ctx and --root are ABI projections"
    ))?;
    Ok(Command::ObjectInstall {
        source,
        manifest,
        tier,
    })
}

pub(crate) fn run_object_check(manifest: &Path) -> Result<(), CliError> {
    let checked =
        cortexfs::object::install::check_object(manifest).map_err(|error| match error {
            InstallError::Invalid(message) => CliError::usage(message),
            InstallError::Unavailable(message) => CliError::unavailable(message),
        })?;
    print_line(&format!(
        "valid {}/{}",
        checked.class().as_str(),
        terminal_safe_text(checked.name())
    ))
}

pub(crate) fn run_object_install(
    source: &Path,
    manifest: &Path,
    tier: InstallTier,
) -> Result<(), CliError> {
    let installed =
        cortexfs::object::install::install_object(source, manifest, tier).map_err(|error| {
            match error {
                InstallError::Invalid(message) => CliError::usage(message),
                InstallError::Unavailable(message) => CliError::unavailable(message),
            }
        })?;
    print_line(&format!(
        "installed {}/{}",
        installed.class.as_str(),
        terminal_safe_text(&installed.name)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(values: &[&str]) -> Result<Command, CliError> {
        parse_object_command(values.iter().map(|value| (*value).to_owned()))
    }

    #[test]
    fn parses_default_user_tier() {
        assert!(
            parse(&["install", "--source", "/source", "tool.json"]).is_ok_and(|command| {
                matches!(
                    command,
                    Command::ObjectInstall {
                        source,
                        manifest,
                        tier: InstallTier::User,
                    } if source.as_path() == Path::new("/source")
                        && manifest.as_path() == Path::new("tool.json")
                )
            })
        );
    }

    #[test]
    fn parses_read_only_check_without_source() {
        assert!(parse(&["check", "tool.json"]).is_ok_and(|command| {
            matches!(command, Command::ObjectCheck { manifest } if manifest == Path::new("tool.json"))
        }));
    }

    #[test]
    fn check_rejects_install_flags_and_extra_arguments() {
        for args in [
            &["check", "--source", "/source", "tool.json"][..],
            &["check", "--tier", "system", "tool.json"][..],
            &["check", "--unknown"][..],
            &["check", "one.json", "two.json"][..],
        ] {
            assert!(parse(args).is_err_and(|error| error.message.contains("object check")));
        }
    }

    #[test]
    fn parses_system_tier_in_any_option_order() {
        assert!(
            parse(&[
                "install",
                "--tier",
                "system",
                "agent.json",
                "--source",
                "/durable",
            ])
            .is_ok_and(|command| {
                matches!(
                    command,
                    Command::ObjectInstall {
                        source,
                        manifest,
                        tier: InstallTier::System,
                    } if source.as_path() == Path::new("/durable")
                        && manifest.as_path() == Path::new("agent.json")
                )
            })
        );
    }

    #[test]
    fn requires_source_and_manifest() {
        assert!(
            parse(&["install", "tool.json"])
                .is_err_and(|error| { error.message.contains("requires --source") })
        );
        assert!(
            parse(&["install", "--source", "/source"])
                .is_err_and(|error| { error.message.contains("requires a manifest") })
        );
    }

    #[test]
    fn rejects_duplicate_source() {
        assert!(
            parse(&[
                "install",
                "--source",
                "/one",
                "--source",
                "/two",
                "tool.json",
            ])
            .is_err_and(|error| error.message.contains("--source specified twice"))
        );
    }

    #[test]
    fn rejects_duplicate_or_invalid_tier() {
        assert!(
            parse(&[
                "install",
                "--source",
                "/source",
                "--tier",
                "user",
                "--tier",
                "system",
                "tool.json",
            ])
            .is_err_and(|error| error.message.contains("--tier specified twice"))
        );
        assert!(
            parse(&[
                "install",
                "--source",
                "/source",
                "--tier",
                "local",
                "tool.json",
            ])
            .is_err_and(|error| error.message.contains("--tier expects user or system"))
        );
    }

    #[test]
    fn rejects_unsupported_action_or_flag() {
        assert!(
            parse(&["remove"])
                .is_err_and(|error| error.message.contains("expects check, install, or residue"))
        );
        assert!(
            parse(&["install", "--unknown"]).is_err_and(|error| {
                error.message.contains("unexpected object install argument")
            })
        );
    }

    #[test]
    fn rejects_second_manifest() {
        assert!(
            parse(&["install", "--source", "/source", "one.json", "two.json",])
                .is_err_and(|error| error.message.contains("accepts one manifest path"))
        );
    }
}
