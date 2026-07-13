use crate::{
    CliError, Command, ObjectClass, print_line, required_arg, terminal_safe_field,
    terminal_safe_text,
};
use cortexfs::object::install::{InstallError, InstallTier};
use cortexfs::object::replace::ReplaceMode;
use std::path::{Path, PathBuf};

pub(crate) fn parse_object_command(
    mut values: impl Iterator<Item = String>,
) -> Result<Command, CliError> {
    let action = required_arg(
        &mut values,
        "object requires check, inspect, install, replace, upgrade, rollback, uninstall, or residue",
    )?;
    if action == "check" {
        let manifest = PathBuf::from(required_arg(
            &mut values,
            "object check requires a manifest path",
        )?);
        if manifest.as_os_str().to_string_lossy().starts_with('-') {
            return Err(CliError::usage(format!(
                "unexpected object check argument: {}",
                terminal_safe_field(&manifest.display().to_string())
            )));
        }
        if let Some(value) = values.next() {
            return Err(CliError::usage(format!(
                "unexpected object check argument: {}",
                terminal_safe_field(&value)
            )));
        }
        return Ok(Command::ObjectCheck { manifest });
    }
    if action == "residue" {
        return crate::residue::parse_object_residue_command(values);
    }
    if action == "inspect" {
        return parse_object_inspect_command(values);
    }
    let replace_mode = match action.as_str() {
        "replace" => Some(ReplaceMode::Replace),
        "upgrade" => Some(ReplaceMode::Upgrade),
        "rollback" => Some(ReplaceMode::Rollback),
        _ => None,
    };
    if let Some(mode) = replace_mode {
        return parse_object_replace_command(mode, values);
    }
    if action == "uninstall" {
        return parse_object_uninstall_command(values);
    }
    if action != "install" {
        return Err(CliError::usage(
            "object expects check, inspect, install, replace, upgrade, rollback, uninstall, or residue",
        ));
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
                let value = required_arg(&mut values, "--tier requires user or system")?;
                tier = InstallTier::parse(&value)
                    .ok_or_else(|| CliError::usage("--tier expects user or system"))?;
            }
            _ if value.starts_with('-') => {
                return Err(CliError::usage(format!(
                    "unexpected object install argument: {}",
                    terminal_safe_field(&value)
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

fn replace_words(mode: ReplaceMode) -> (&'static str, &'static str, &'static str) {
    match mode {
        ReplaceMode::Replace => ("replace", "would-replace", "replaced"),
        ReplaceMode::Upgrade => ("upgrade", "would-upgrade", "upgraded"),
        ReplaceMode::Rollback => ("rollback", "would-rollback", "rolled-back"),
    }
}

fn parse_object_replace_command(
    mode: ReplaceMode,
    mut values: impl Iterator<Item = String>,
) -> Result<Command, CliError> {
    let (action, _dry_run, _applied) = replace_words(mode);
    let mut source = None;
    let mut manifest = None;
    let mut tier = InstallTier::User;
    let mut tier_seen = false;
    let mut yes = false;
    while let Some(value) = values.next() {
        match value.as_str() {
            "--source" => {
                if source.is_some() {
                    return Err(CliError::usage(format!(
                        "object {action} --source specified twice"
                    )));
                }
                let message = format!("object {action} --source requires a durable backing path");
                let value = required_arg(&mut values, &message)?;
                if value.starts_with('-') {
                    return Err(CliError::usage(message));
                }
                source = Some(PathBuf::from(value));
            }
            "--tier" => {
                if tier_seen {
                    return Err(CliError::usage(format!(
                        "object {action} --tier specified twice"
                    )));
                }
                tier_seen = true;
                let message = format!("object {action} --tier requires user or system");
                let value = required_arg(&mut values, &message)?;
                tier = InstallTier::parse(&value).ok_or_else(|| {
                    CliError::usage(format!("object {action} --tier expects user or system"))
                })?;
            }
            "--yes" => {
                if yes {
                    return Err(CliError::usage(format!(
                        "object {action} --yes specified twice"
                    )));
                }
                yes = true;
            }
            _ if value.starts_with('-') => {
                return Err(CliError::usage(format!(
                    "unexpected object {action} argument: {}",
                    terminal_safe_field(&value)
                )));
            }
            _ if manifest.is_none() => manifest = Some(PathBuf::from(value)),
            _ => {
                return Err(CliError::usage(format!(
                    "object {action} accepts one manifest path"
                )));
            }
        }
    }
    Ok(Command::ObjectReplace {
        source: source.ok_or_else(|| {
            CliError::usage(format!(
                "object {action} requires --source PATH for the durable backing tree; /ctx and --root are ABI projections"
            ))
        })?,
        manifest: manifest.ok_or_else(|| {
            CliError::usage(format!("object {action} requires a manifest path"))
        })?,
        tier,
        mode,
        yes,
    })
}

fn parse_object_uninstall_command(
    mut values: impl Iterator<Item = String>,
) -> Result<Command, CliError> {
    let mut source = None;
    let mut class = None;
    let mut name = None;
    let mut tier = InstallTier::User;
    let mut tier_seen = false;
    let mut yes = false;
    while let Some(value) = values.next() {
        match value.as_str() {
            "--source" => {
                if source.is_some() {
                    return Err(CliError::usage("object uninstall --source specified twice"));
                }
                let value = required_arg(
                    &mut values,
                    "object uninstall --source requires a durable backing path",
                )?;
                if value.starts_with('-') {
                    return Err(CliError::usage(
                        "object uninstall --source requires a durable backing path",
                    ));
                }
                source = Some(PathBuf::from(value));
            }
            "--tier" => {
                if tier_seen {
                    return Err(CliError::usage("object uninstall --tier specified twice"));
                }
                tier_seen = true;
                let value = required_arg(
                    &mut values,
                    "object uninstall --tier requires user or system",
                )?;
                tier = InstallTier::parse(&value).ok_or_else(|| {
                    CliError::usage("object uninstall --tier expects user or system")
                })?;
            }
            "--yes" => {
                if yes {
                    return Err(CliError::usage("object uninstall --yes specified twice"));
                }
                yes = true;
            }
            _ if value.starts_with('-') => {
                return Err(CliError::usage(format!(
                    "unexpected object uninstall argument: {}",
                    terminal_safe_field(&value)
                )));
            }
            _ if class.is_none() => {
                class = match ObjectClass::parse(&value) {
                    Some(class @ (ObjectClass::Tool | ObjectClass::Agent)) => Some(class),
                    _ => {
                        return Err(CliError::usage(
                            "object uninstall CLASS expects tool or agent",
                        ));
                    }
                };
            }
            _ if name.is_none() => name = Some(value),
            _ => return Err(CliError::usage("object uninstall accepts CLASS and NAME")),
        }
    }
    Ok(Command::ObjectUninstall {
        source: source.ok_or_else(|| {
            CliError::usage(
                "object uninstall requires --source PATH for the durable backing tree; /ctx and --root are ABI projections",
            )
        })?,
        class: class
            .ok_or_else(|| CliError::usage("object uninstall requires CLASS: tool or agent"))?,
        name: name.ok_or_else(|| CliError::usage("object uninstall requires an object NAME"))?,
        tier,
        yes,
    })
}

fn parse_object_inspect_command(
    mut values: impl Iterator<Item = String>,
) -> Result<Command, CliError> {
    let mut source = None;
    let mut class = None;
    let mut name = None;
    let mut tier = InstallTier::User;
    let mut tier_seen = false;
    while let Some(value) = values.next() {
        match value.as_str() {
            "--source" => {
                if source.is_some() {
                    return Err(CliError::usage("object inspect --source specified twice"));
                }
                source = Some(PathBuf::from(required_arg(
                    &mut values,
                    "object inspect --source requires a durable backing path",
                )?));
            }
            "--tier" => {
                if tier_seen {
                    return Err(CliError::usage("object inspect --tier specified twice"));
                }
                tier_seen = true;
                let value = required_arg(&mut values, "--tier requires user or system")?;
                tier = InstallTier::parse(&value)
                    .ok_or_else(|| CliError::usage("--tier expects user or system"))?;
            }
            _ if value.starts_with('-') => {
                return Err(CliError::usage(format!(
                    "unexpected object inspect argument: {}",
                    terminal_safe_field(&value)
                )));
            }
            _ if class.is_none() => {
                class = match ObjectClass::parse(&value) {
                    Some(class @ (ObjectClass::Tool | ObjectClass::Agent)) => Some(class),
                    _ => {
                        return Err(CliError::usage(
                            "object inspect CLASS expects tool or agent",
                        ));
                    }
                };
            }
            _ if name.is_none() => name = Some(value),
            _ => return Err(CliError::usage("object inspect accepts CLASS and NAME")),
        }
    }
    Ok(Command::ObjectInspect {
        source: source.ok_or_else(|| {
            CliError::usage(
                "object inspect requires --source PATH for the durable backing tree; /ctx and --root are ABI projections",
            )
        })?,
        class: class.ok_or_else(|| CliError::usage("object inspect requires CLASS: tool or agent"))?,
        name: name.ok_or_else(|| CliError::usage("object inspect requires an object NAME"))?,
        tier,
    })
}

pub(crate) fn run_object_check(manifest: &Path) -> Result<(), CliError> {
    let checked =
        cortexfs::object::install::check_object(manifest).map_err(|error| match error {
            InstallError::Invalid(message) => CliError::usage(terminal_safe_field(&message)),
            InstallError::Unavailable(message) => {
                CliError::unavailable(terminal_safe_field(&message))
            }
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
                InstallError::Invalid(message) => CliError::usage(terminal_safe_field(&message)),
                InstallError::Unavailable(message) => {
                    CliError::unavailable(terminal_safe_field(&message))
                }
            }
        })?;
    print_line(&format!(
        "installed {}/{}",
        installed.class.as_str(),
        terminal_safe_text(&installed.name)
    ))
}

pub(crate) fn run_object_inspect(
    source: &Path,
    class: ObjectClass,
    name: &str,
    tier: InstallTier,
) -> Result<(), CliError> {
    let inspected =
        cortexfs::object::receipt::inspect_object(source, class, name, tier).map_err(|error| {
            match error {
                InstallError::Invalid(message) => CliError::usage(terminal_safe_field(&message)),
                InstallError::Unavailable(message) => {
                    CliError::unavailable(terminal_safe_field(&message))
                }
            }
        })?;
    let compatibility = inspected
        .object_version()
        .zip(inspected.cortexfs_requirement())
        .map_or_else(String::new, |(version, requirement)| {
            format!(
                " version={} requires-cortexfs={}",
                terminal_safe_field(version),
                terminal_safe_field(requirement)
            )
        });
    print_line(&format!(
        "installed {}/{} tier={} schema={}{compatibility} sha256={} executable={}:{} control={}:{}",
        terminal_safe_text(inspected.class().as_str()),
        terminal_safe_text(inspected.name()),
        terminal_safe_text(inspected.tier().as_str()),
        terminal_safe_text(inspected.object_schema()),
        terminal_safe_text(inspected.sha256()),
        inspected.executable_dev(),
        inspected.executable_ino(),
        inspected.control_dev(),
        inspected.control_ino(),
    ))
}

pub(crate) fn run_object_uninstall(
    source: &Path,
    class: ObjectClass,
    name: &str,
    tier: InstallTier,
    yes: bool,
) -> Result<(), CliError> {
    let inspected = cortexfs::object::uninstall::uninstall_object(source, class, name, tier, yes)
        .map_err(|error| match error {
        InstallError::Invalid(message) => CliError::usage(terminal_safe_field(&message)),
        InstallError::Unavailable(message) => CliError::unavailable(terminal_safe_field(&message)),
    })?;
    let action = if yes {
        "uninstalled"
    } else {
        "would-uninstall"
    };
    print_line(&format!(
        "{action} {}/{} tier={} executable={}:{} control={}:{}",
        terminal_safe_field(inspected.class().as_str()),
        terminal_safe_field(inspected.name()),
        terminal_safe_field(inspected.tier().as_str()),
        inspected.executable_dev(),
        inspected.executable_ino(),
        inspected.control_dev(),
        inspected.control_ino(),
    ))
}

pub(crate) fn run_object_replace(
    source: &Path,
    manifest: &Path,
    tier: InstallTier,
    mode: ReplaceMode,
    yes: bool,
) -> Result<(), CliError> {
    let (_action, dry_run, applied) = replace_words(mode);
    let report = cortexfs::object::replace::replace_object(source, manifest, tier, mode, yes)
        .map_err(|error| match error {
            InstallError::Invalid(message) => CliError::usage(terminal_safe_field(&message)),
            InstallError::Unavailable(message) => {
                CliError::unavailable(terminal_safe_field(&message))
            }
        })?;
    let action = if report.applied { applied } else { dry_run };
    print_line(&format!(
        "{action} {}/{} tier={} from={} to={}",
        terminal_safe_field(report.class.as_str()),
        terminal_safe_field(&report.name),
        terminal_safe_field(report.tier.as_str()),
        terminal_safe_field(report.from_version.as_deref().unwrap_or("legacy")),
        terminal_safe_field(&report.to_version),
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
    fn parses_inspect_with_default_user_tier() {
        assert!(
            parse(&["inspect", "tool", "example.echo", "--source", "/source"]).is_ok_and(
                |command| {
                    matches!(
                        command,
                        Command::ObjectInspect {
                            source,
                            class: ObjectClass::Tool,
                            name,
                            tier: InstallTier::User,
                        } if source == Path::new("/source") && name == "example.echo"
                    )
                }
            )
        );
    }

    #[test]
    fn parses_inspect_system_tier_in_any_option_order() {
        assert!(
            parse(&[
                "inspect",
                "--tier",
                "system",
                "agent",
                "--source",
                "/durable",
                "example-agent",
            ])
            .is_ok_and(|command| {
                matches!(
                    command,
                    Command::ObjectInspect {
                        source,
                        class: ObjectClass::Agent,
                        name,
                        tier: InstallTier::System,
                    } if source == Path::new("/durable") && name == "example-agent"
                )
            })
        );
    }

    #[test]
    fn parses_uninstall_dry_run_with_default_user_tier() {
        assert!(
            parse(&["uninstall", "tool", "example.echo", "--source", "/source"]).is_ok_and(
                |command| {
                    matches!(
                        command,
                        Command::ObjectUninstall {
                            source,
                            class: ObjectClass::Tool,
                            name,
                            tier: InstallTier::User,
                            yes: false,
                        } if source == Path::new("/source") && name == "example.echo"
                    )
                }
            )
        );
    }

    #[test]
    fn parses_applied_system_uninstall_in_any_option_order() {
        assert!(
            parse(&[
                "uninstall",
                "--tier",
                "system",
                "agent",
                "--yes",
                "example-agent",
                "--source",
                "/durable",
            ])
            .is_ok_and(|command| {
                matches!(
                    command,
                    Command::ObjectUninstall {
                        source,
                        class: ObjectClass::Agent,
                        name,
                        tier: InstallTier::System,
                        yes: true,
                    } if source == Path::new("/durable") && name == "example-agent"
                )
            })
        );
    }

    #[test]
    fn parses_all_replace_modes_as_default_user_dry_runs() {
        for (action, expected_mode) in [
            ("replace", ReplaceMode::Replace),
            ("upgrade", ReplaceMode::Upgrade),
            ("rollback", ReplaceMode::Rollback),
        ] {
            assert!(
                parse(&[action, "--source", "/source", "tool.json"]).is_ok_and(|command| {
                    matches!(
                        command,
                        Command::ObjectReplace {
                            source,
                            manifest,
                            tier: InstallTier::User,
                            mode,
                            yes: false,
                        } if source == Path::new("/source")
                            && manifest == Path::new("tool.json")
                            && mode == expected_mode
                    )
                }),
                "{action}"
            );
        }
    }

    #[test]
    fn parses_applied_system_upgrade_in_any_option_order() {
        assert!(
            parse(&[
                "upgrade",
                "--tier",
                "system",
                "tool.json",
                "--yes",
                "--source",
                "/durable",
            ])
            .is_ok_and(|command| {
                matches!(
                    command,
                    Command::ObjectReplace {
                        source,
                        manifest,
                        tier: InstallTier::System,
                        mode: ReplaceMode::Upgrade,
                        yes: true,
                    } if source == Path::new("/durable")
                        && manifest == Path::new("tool.json")
                )
            })
        );
    }

    #[test]
    fn replace_commands_reject_missing_duplicate_unknown_and_extra_arguments() {
        for args in [
            &["replace", "tool.json"][..],
            &["replace", "--source", "/source"][..],
            &["replace", "--source"][..],
            &["replace", "--source", "--yes", "tool.json"][..],
            &["replace", "--source", "-source", "tool.json"][..],
            &["replace", "--source", "/source", "-tool.json"][..],
            &[
                "replace",
                "--source",
                "/one",
                "--source",
                "/two",
                "tool.json",
            ][..],
            &[
                "replace",
                "--source",
                "/source",
                "--tier",
                "user",
                "--tier",
                "system",
                "tool.json",
            ][..],
            &[
                "replace",
                "--source",
                "/source",
                "tool.json",
                "--yes",
                "--yes",
            ][..],
            &["replace", "--source", "/source", "--unknown", "tool.json"][..],
            &["replace", "--source", "/source", "one.json", "two.json"][..],
            &[
                "replace",
                "--source",
                "/source",
                "--tier",
                "local",
                "tool.json",
            ][..],
        ] {
            assert!(parse(args).is_err_and(|error| error.message.contains("object replace")));
        }
    }

    #[test]
    fn replace_command_escapes_multiline_unknown_argument() -> Result<(), &'static str> {
        let Err(error) = parse(&[
            "rollback",
            "--source",
            "/source",
            "tool.json",
            "--evil\nINJECTED",
        ]) else {
            return Err("multiline option was accepted");
        };
        assert_eq!(error.message.lines().count(), 1);
        assert!(error.message.contains("--evil\\nINJECTED"));
        Ok(())
    }

    #[test]
    fn uninstall_requires_source_class_and_name() {
        for args in [
            &["uninstall", "tool", "example.echo"][..],
            &["uninstall", "--source", "/source"][..],
            &["uninstall", "--source", "/source", "tool"][..],
        ] {
            assert!(parse(args).is_err_and(|error| error.message.contains("object uninstall")));
        }
    }

    #[test]
    fn uninstall_rejects_missing_duplicate_unknown_invalid_and_extra_arguments() {
        for args in [
            &["uninstall", "--source"][..],
            &["uninstall", "--source", "--yes", "tool", "example.echo"][..],
            &["uninstall", "--tier"][..],
            &[
                "uninstall",
                "--source",
                "/one",
                "--source",
                "/two",
                "tool",
                "example.echo",
            ][..],
            &[
                "uninstall",
                "--source",
                "/source",
                "--tier",
                "user",
                "--tier",
                "system",
                "tool",
                "example.echo",
            ][..],
            &[
                "uninstall",
                "--source",
                "/source",
                "tool",
                "example.echo",
                "--yes",
                "--yes",
            ][..],
            &[
                "uninstall",
                "--source",
                "/source",
                "--unknown",
                "tool",
                "example.echo",
            ][..],
            &["uninstall", "--source", "/source", "model", "example"][..],
            &[
                "uninstall",
                "--source",
                "/source",
                "tool",
                "example.echo",
                "extra",
            ][..],
            &[
                "uninstall",
                "--source",
                "/source",
                "tool",
                "example.echo",
                "--tier",
                "local",
            ][..],
        ] {
            assert!(parse(args).is_err_and(|error| error.message.contains("object uninstall")));
        }
    }

    #[test]
    fn uninstall_escapes_multiline_unknown_argument() -> Result<(), &'static str> {
        let Err(error) = parse(&[
            "uninstall",
            "--source",
            "/source",
            "tool",
            "example.echo",
            "--evil\nINJECTED",
        ]) else {
            return Err("multiline option was accepted");
        };
        assert_eq!(error.message.lines().count(), 1);
        assert!(error.message.contains("--evil\\nINJECTED"));
        Ok(())
    }

    #[test]
    fn inspect_requires_source_class_and_name() {
        for args in [
            &["inspect", "tool", "example.echo"][..],
            &["inspect", "--source", "/source"][..],
            &["inspect", "--source", "/source", "tool"][..],
        ] {
            assert!(parse(args).is_err_and(|error| error.message.contains("object inspect")));
        }
    }

    #[test]
    fn inspect_rejects_missing_option_values_and_invalid_tier() {
        for args in [
            &["inspect", "--source"][..],
            &["inspect", "--tier"][..],
            &[
                "inspect",
                "--source",
                "/source",
                "tool",
                "example.echo",
                "--tier",
                "local",
            ][..],
        ] {
            assert!(parse(args).is_err());
        }
    }

    #[test]
    fn inspect_rejects_duplicate_unknown_invalid_and_extra_arguments() {
        for args in [
            &[
                "inspect",
                "--source",
                "/one",
                "--source",
                "/two",
                "tool",
                "example.echo",
            ][..],
            &[
                "inspect",
                "--source",
                "/source",
                "--tier",
                "user",
                "--tier",
                "system",
                "tool",
                "example.echo",
            ][..],
            &[
                "inspect",
                "--source",
                "/source",
                "--unknown",
                "tool",
                "example.echo",
            ][..],
            &["inspect", "--source", "/source", "model", "example"][..],
            &[
                "inspect",
                "--source",
                "/source",
                "tool",
                "example.echo",
                "extra",
            ][..],
        ] {
            assert!(parse(args).is_err_and(|error| error.message.contains("object inspect")));
        }
    }

    #[test]
    fn inspect_escapes_multiline_unknown_argument() -> Result<(), &'static str> {
        let Err(error) = parse(&[
            "inspect",
            "--source",
            "/source",
            "tool",
            "example.echo",
            "--evil\nINJECTED",
        ]) else {
            return Err("multiline option was accepted");
        };
        assert_eq!(error.message.lines().count(), 1);
        assert!(error.message.contains("--evil\\nINJECTED"));
        Ok(())
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
        assert!(parse(&["remove"]).is_err_and(|error| {
            error
                .message
                .contains("expects check, inspect, install, replace, upgrade, rollback, uninstall, or residue")
        }));
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
