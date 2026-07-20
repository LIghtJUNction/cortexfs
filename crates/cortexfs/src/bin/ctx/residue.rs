use crate::{CliError, Command, print_line, required_arg, terminal_safe_field};
use cortexfs::object::residue::{ResidueCleanupReport, ResidueError, ResidueReport};
use std::path::{Path, PathBuf};

pub(crate) fn parse_object_residue_command(
    mut values: impl Iterator<Item = String>,
) -> Result<Command, CliError> {
    let action = required_arg(&mut values, "object residue requires audit or cleanup")?;
    match action.as_str() {
        "audit" => parse_residue_audit(values),
        "cleanup" => parse_residue_cleanup(values),
        "--help" | "-h" => {
            if let Some(value) = values.next() {
                return Err(CliError::usage(format!(
                    "unexpected object residue help argument: {}",
                    terminal_safe_field(&value)
                )));
            }
            Ok(Command::HelpTopic("object".to_owned()))
        }
        _ => Err(CliError::usage("object residue expects audit or cleanup")),
    }
}

fn parse_residue_audit(mut values: impl Iterator<Item = String>) -> Result<Command, CliError> {
    let mut source = None;
    while let Some(value) = values.next() {
        match value.as_str() {
            "--source" if source.is_none() => {
                source = Some(PathBuf::from(required_arg(
                    &mut values,
                    "object residue audit --source requires a durable backing path",
                )?));
            }
            "--source" => {
                return Err(CliError::usage(
                    "object residue audit --source specified twice",
                ));
            }
            _ => {
                return Err(CliError::usage(format!(
                    "unexpected object residue audit argument: {}",
                    terminal_safe_field(&value)
                )));
            }
        }
    }
    let source = source.ok_or_else(|| {
        CliError::usage("object residue audit requires --source PATH for the durable backing tree")
    })?;
    Ok(Command::ObjectResidueAudit { source })
}

fn parse_residue_cleanup(mut values: impl Iterator<Item = String>) -> Result<Command, CliError> {
    let mut source = None;
    let mut path = None;
    let mut dev = None;
    let mut ino = None;
    let mut yes = false;
    while let Some(value) = values.next() {
        match value.as_str() {
            "--source" if source.is_none() => {
                source = Some(PathBuf::from(required_arg(
                    &mut values,
                    "object residue cleanup --source requires a durable backing path",
                )?));
            }
            "--path" if path.is_none() => {
                path = Some(PathBuf::from(required_arg(
                    &mut values,
                    "object residue cleanup --path requires a relative residue path",
                )?));
            }
            "--dev" if dev.is_none() => {
                let value = required_arg(
                    &mut values,
                    "object residue cleanup --dev requires a device number",
                )?;
                dev = Some(value.parse::<u64>().map_err(|_error| {
                    CliError::usage("object residue cleanup --dev expects an unsigned integer")
                })?);
            }
            "--ino" if ino.is_none() => {
                let value = required_arg(
                    &mut values,
                    "object residue cleanup --ino requires an inode number",
                )?;
                ino = Some(value.parse::<u64>().map_err(|_error| {
                    CliError::usage("object residue cleanup --ino expects an unsigned integer")
                })?);
            }
            "--yes" if !yes => yes = true,
            "--source" | "--path" | "--dev" | "--ino" | "--yes" => {
                return Err(CliError::usage(format!(
                    "object residue cleanup {} specified twice",
                    terminal_safe_field(&value)
                )));
            }
            _ => {
                return Err(CliError::usage(format!(
                    "unexpected object residue cleanup argument: {}",
                    terminal_safe_field(&value)
                )));
            }
        }
    }
    Ok(Command::ObjectResidueCleanup {
        source: source
            .ok_or_else(|| CliError::usage("object residue cleanup requires --source PATH"))?,
        path: path.ok_or_else(|| CliError::usage("object residue cleanup requires --path REL"))?,
        dev: dev.ok_or_else(|| CliError::usage("object residue cleanup requires --dev DEV"))?,
        ino: ino.ok_or_else(|| CliError::usage("object residue cleanup requires --ino INO"))?,
        yes,
    })
}

pub(crate) fn run_object_residue_audit(source: &Path) -> Result<(), CliError> {
    let reports = cortexfs::object::residue::audit_residue(source).map_err(residue_cli_error)?;
    for report in reports {
        print_line(&format_residue_report(&report))?;
    }
    Ok(())
}

pub(crate) fn run_object_residue_cleanup(
    source: &Path,
    path: &Path,
    dev: u64,
    ino: u64,
    yes: bool,
) -> Result<(), CliError> {
    let report = cortexfs::object::residue::cleanup_residue(source, path, dev, ino, yes)
        .map_err(residue_cli_error)?;
    print_line(&format_cleanup_report(&report))
}

fn format_residue_report(report: &ResidueReport) -> String {
    format!(
        "residue kind={} path={} dev={} ino={} type={} state={} cleanup={}",
        report.kind.as_str(),
        terminal_safe_field(&report.path.to_string_lossy()),
        report.dev,
        report.ino,
        report.file_kind.as_str(),
        report.occupancy.as_str(),
        report.eligibility.as_str(),
    )
}

fn format_cleanup_report(report: &ResidueCleanupReport) -> String {
    format!(
        "{} path={} dev={} ino={} entries={}",
        if report.applied {
            "cleaned"
        } else {
            "would-clean"
        },
        terminal_safe_field(&report.path.to_string_lossy()),
        report.dev,
        report.ino,
        report.entries,
    )
}

fn residue_cli_error(error: ResidueError) -> CliError {
    match error {
        ResidueError::Invalid(message) => CliError::usage(terminal_safe_field(&message)),
        ResidueError::Unavailable(message) => CliError::unavailable(terminal_safe_field(&message)),
        ResidueError::Conflict(conflict) => {
            let message = ResidueError::Conflict(conflict).to_string();
            CliError::unavailable(terminal_safe_field(&message))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{format_cleanup_report, format_residue_report, residue_cli_error};
    use crate::{CliError, Command};
    use cortexfs::object::residue::{
        ResidueCleanupReport, ResidueConflict, ResidueEligibility, ResidueError, ResidueFileKind,
        ResidueKind, ResidueOccupancy, ResidueReport,
    };
    use std::path::{Path, PathBuf};

    fn parse(values: &[&str]) -> Result<Command, CliError> {
        crate::install::parse_object_command(values.iter().map(|value| (*value).to_owned()))
    }

    #[test]
    fn parses_residue_audit() {
        assert!(
            parse(&["residue", "audit", "--source", "/durable"]).is_ok_and(|command| {
                matches!(
                    command,
                    Command::ObjectResidueAudit { source }
                        if source == Path::new("/durable")
                )
            })
        );
    }

    #[test]
    fn parses_residue_cleanup_as_dry_run_by_default() {
        assert!(
            parse(&[
                "residue",
                "cleanup",
                "--source",
                "/durable",
                "--path",
                "tool/.cortexfs-install-1",
                "--dev",
                "9",
                "--ino",
                "42",
            ])
            .is_ok_and(|command| {
                matches!(
                    command,
                    Command::ObjectResidueCleanup {
                        source,
                        path,
                        dev: 9,
                        ino: 42,
                        yes: false,
                    } if source == Path::new("/durable")
                        && path == Path::new("tool/.cortexfs-install-1")
                )
            })
        );
    }

    #[test]
    fn parses_residue_cleanup_yes() {
        assert!(
            parse(&[
                "residue",
                "cleanup",
                "--dev",
                "9",
                "--yes",
                "--path",
                "home/1000/agent/.cortexfs-install-2",
                "--ino",
                "42",
                "--source",
                "/durable",
            ])
            .is_ok_and(|command| {
                matches!(command, Command::ObjectResidueCleanup { yes: true, .. })
            })
        );
    }

    #[test]
    fn parses_residue_help_as_object_topic() {
        assert!(parse(&["residue", "--help"]).is_ok_and(|command| {
            matches!(command, Command::HelpTopic(topic) if topic == "object")
        }));
    }

    #[test]
    fn residue_rejects_missing_or_unknown_action() {
        for args in [
            &["residue"][..],
            &["residue", "remove"][..],
            &["residue", "audit"][..],
            &["residue", "cleanup", "--source", "/durable"][..],
        ] {
            assert!(parse(args).is_err_and(|error| error.message.contains("object residue")));
        }
    }

    #[test]
    fn residue_cli_surfaces_escape_control_characters() -> Result<(), &'static str> {
        let path = PathBuf::from("tool/雪-\n\r\t\u{1b}");
        let audit = format_residue_report(&ResidueReport {
            kind: ResidueKind::Install,
            path: path.clone(),
            dev: 7,
            ino: 11,
            file_kind: ResidueFileKind::Directory,
            occupancy: ResidueOccupancy::Occupied,
            eligibility: ResidueEligibility::Eligible,
        });
        let cleanup = format_cleanup_report(&ResidueCleanupReport {
            path: path.clone(),
            dev: 7,
            ino: 11,
            entries: 3,
            applied: false,
        });
        let conflict = residue_cli_error(ResidueError::Conflict(ResidueConflict {
            path,
            dev: 7,
            ino: 11,
            quarantine: None,
            stage: "test",
            detail: "detail\n\r\t\u{1b}".to_owned(),
        }))
        .message;

        for output in [&audit, &cleanup, &conflict] {
            assert!(!output.chars().any(char::is_control));
            assert!(output.contains('雪'));
            for escaped in [r"\n", r"\r", r"\t", r"\u{1b}"] {
                assert!(output.contains(escaped));
            }
        }

        let Err(parsed) = parse(&["residue", "audit", "未知\n\r\t"]) else {
            return Err("unknown residue argument was accepted");
        };
        assert!(!parsed.message.chars().any(char::is_control));
        assert!(parsed.message.contains("未知"));
        for escaped in [r"\n", r"\r", r"\t"] {
            assert!(parsed.message.contains(escaped));
        }
        Ok(())
    }
}
