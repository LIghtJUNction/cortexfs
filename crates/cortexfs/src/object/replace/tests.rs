use super::{FaultPoint, ReplaceFault, ReplaceMode, replace_object};
use crate::ObjectClass;
use crate::object::install::{InstallError, InstallTier, install_object};
use crate::object::receipt::inspect_object;
use crate::object::residue::{audit_residue, cleanup_residue};

use serde_json::json;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::rc::Rc;

const NAME: &str = "example.echo";
type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

struct Fixture {
    root: tempfile::TempDir,
    old_executable: u64,
    old_control: u64,
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            let _ignored = write!(output, "{byte:02x}");
            output
        })
}

fn manifest(
    root: &Path,
    file: &str,
    schema: &str,
    version: Option<&str>,
    bytes: &[u8],
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let artifact = root.join(format!("{file}.sh"));
    fs::write(&artifact, bytes)?;
    fs::set_permissions(&artifact, fs::Permissions::from_mode(0o755))?;
    let mut value = json!({
        "schema": schema,
        "class": "tool",
        "name": NAME,
        "executable": { "path": artifact, "sha256": digest(bytes) },
        "controls": {
            "description": "echo",
            "schema": r#"{"type":"object"}"#,
            "cap": "text",
            "policy": ""
        }
    });
    if let Some(version) = version {
        let object = value
            .as_object_mut()
            .ok_or("manifest fixture is not an object")?;
        object.insert("version".to_owned(), json!(version));
        object.insert("compatibility".to_owned(), json!({ "cortexfs": "*" }));
    }
    let path = root.join(format!("{file}.json"));
    fs::write(&path, serde_json::to_vec(&value)?)?;
    Ok(path)
}

fn fixture(schema: &str, version: Option<&str>) -> Result<Fixture, Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::create_dir_all(root.path().join("tool"))?;
    let old = manifest(
        root.path(),
        "old",
        schema,
        version,
        b"#!/bin/sh\nprintf old\n",
    )?;
    install_object(root.path(), &old, InstallTier::System)?;
    for residue in audit_residue(root.path())? {
        cleanup_residue(root.path(), &residue.path, residue.dev, residue.ino, true)?;
    }
    let old_executable = fs::symlink_metadata(root.path().join("tool").join(NAME))?.ino();
    let old_control =
        fs::symlink_metadata(root.path().join("tool").join(format!("{NAME}.d")))?.ino();
    Ok(Fixture {
        root,
        old_executable,
        old_control,
    })
}

fn class_snapshot(root: &Path) -> TestResult<Vec<(OsString, u64)>> {
    let mut entries = fs::read_dir(root.join("tool"))?
        .map(|entry| {
            let entry = entry?;
            Ok((entry.file_name(), entry.metadata()?.ino()))
        })
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(entries)
}

fn candidate(
    fixture: &Fixture,
    file: &str,
    version: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    manifest(
        fixture.root.path(),
        file,
        "cortexfs.object/v2",
        Some(version),
        format!("#!/bin/sh\nprintf {version}\n").as_bytes(),
    )
}

#[test]
fn dry_run_validates_without_creating_stage() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture("cortexfs.object/v2", Some("1.0.0"))?;
    let candidate = candidate(&fixture, "candidate", "2.0.0")?;
    let before = class_snapshot(fixture.root.path())?;

    let report = replace_object(
        fixture.root.path(),
        &candidate,
        InstallTier::System,
        ReplaceMode::Upgrade,
        false,
    )?;

    assert!(!report.applied);
    assert_eq!(class_snapshot(fixture.root.path())?, before);
    Ok(())
}

#[test]
fn replace_allows_v1_to_v2() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture("cortexfs.object/v1", None)?;
    let candidate = candidate(&fixture, "candidate", "2.0.0")?;

    let report = replace_object(
        fixture.root.path(),
        &candidate,
        InstallTier::System,
        ReplaceMode::Replace,
        true,
    )?;

    assert_eq!(report.from_version, None);
    assert_eq!(
        inspect_object(
            fixture.root.path(),
            ObjectClass::Tool,
            NAME,
            InstallTier::System,
        )?
        .object_version(),
        Some("2.0.0")
    );
    Ok(())
}

#[test]
fn upgrade_publishes_executable_last_and_cleans_after_commit()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture("cortexfs.object/v2", Some("1.0.0"))?;
    let candidate = candidate(&fixture, "candidate", "2.0.0")?;
    let events = Rc::new(RefCell::new(Vec::new()));
    let _fault = ReplaceFault {
        events: Some(Rc::clone(&events)),
        ..ReplaceFault::default()
    }
    .install();

    replace_object(
        fixture.root.path(),
        &candidate,
        InstallTier::System,
        ReplaceMode::Upgrade,
        true,
    )?;

    assert_eq!(
        *events.borrow(),
        [
            "old-executable",
            "old-control",
            "new-control",
            "new-executable",
            "commit",
            "cleanup",
        ]
    );
    Ok(())
}

#[test]
fn rollback_allows_strictly_lower_v2() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture("cortexfs.object/v2", Some("2.0.0"))?;
    let candidate = candidate(&fixture, "candidate", "1.0.0")?;

    let report = replace_object(
        fixture.root.path(),
        &candidate,
        InstallTier::System,
        ReplaceMode::Rollback,
        true,
    )?;

    assert_eq!(report.from_version.as_deref(), Some("2.0.0"));
    assert_eq!(report.to_version, "1.0.0");
    Ok(())
}

#[test]
fn invalid_version_directions_are_zero_write() -> TestResult<()> {
    for (mode, current, candidate_version) in [
        (ReplaceMode::Upgrade, "2.0.0", "1.0.0"),
        (ReplaceMode::Upgrade, "1.0.0", "1.0.0"),
        (ReplaceMode::Rollback, "1.0.0", "1.0.0"),
        (ReplaceMode::Rollback, "1.0.0", "2.0.0"),
    ] {
        let fixture = fixture("cortexfs.object/v2", Some(current))?;
        let candidate = candidate(&fixture, "candidate", candidate_version)?;
        let before = class_snapshot(fixture.root.path())?;

        let result = replace_object(
            fixture.root.path(),
            &candidate,
            InstallTier::System,
            mode,
            true,
        );

        assert!(
            matches!(result, Err(InstallError::Invalid(_))),
            "{mode:?} unexpectedly accepted {current} -> {candidate_version}"
        );
        assert_eq!(class_snapshot(fixture.root.path())?, before);
    }
    Ok(())
}

#[test]
fn legacy_candidate_is_zero_write() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture("cortexfs.object/v1", None)?;
    let candidate = manifest(
        fixture.root.path(),
        "candidate",
        "cortexfs.object/v1",
        None,
        b"#!/bin/sh\nprintf candidate\n",
    )?;
    let before = class_snapshot(fixture.root.path())?;

    let result = replace_object(
        fixture.root.path(),
        &candidate,
        InstallTier::System,
        ReplaceMode::Replace,
        true,
    );

    assert!(matches!(result, Err(InstallError::Invalid(_))));
    assert_eq!(class_snapshot(fixture.root.path())?, before);
    Ok(())
}

struct ForeignOutcome {
    fixture: Fixture,
    stage: PathBuf,
    executable: u64,
    control: u64,
}

fn audit_outcome(fixture: Fixture) -> TestResult<ForeignOutcome> {
    let reports = audit_residue(fixture.root.path())?;
    assert_eq!(reports.len(), 1);
    let report = reports
        .first()
        .ok_or("replacement residue was not audited")?;
    let stage = fixture.root.path().join(&report.path);
    let stage_metadata = fs::symlink_metadata(&stage)?;
    assert_eq!(
        (report.dev, report.ino),
        (stage_metadata.dev(), stage_metadata.ino())
    );
    let executable = fs::symlink_metadata(fixture.root.path().join("tool").join(NAME))?.ino();
    let control =
        fs::symlink_metadata(fixture.root.path().join("tool").join(format!("{NAME}.d")))?.ino();
    Ok(ForeignOutcome {
        fixture,
        stage,
        executable,
        control,
    })
}

fn foreign(point: FaultPoint) -> Result<ForeignOutcome, Box<dyn std::error::Error>> {
    let fixture = fixture("cortexfs.object/v2", Some("1.0.0"))?;
    let candidate = candidate(&fixture, "candidate", "2.0.0")?;
    let _fault = ReplaceFault {
        foreign: Some(point),
        ..ReplaceFault::default()
    }
    .install();
    let result = replace_object(
        fixture.root.path(),
        &candidate,
        InstallTier::System,
        ReplaceMode::Upgrade,
        true,
    );
    assert!(matches!(result, Err(InstallError::Unavailable(_))));
    audit_outcome(fixture)
}

fn recreated_source(point: FaultPoint) -> TestResult<ForeignOutcome> {
    let fixture = fixture("cortexfs.object/v2", Some("1.0.0"))?;
    let candidate = candidate(&fixture, "candidate", "2.0.0")?;
    let _fault = ReplaceFault {
        recreate_source: Some(point),
        ..ReplaceFault::default()
    }
    .install();

    let result = replace_object(
        fixture.root.path(),
        &candidate,
        InstallTier::System,
        ReplaceMode::Upgrade,
        true,
    );

    assert!(matches!(
        result,
        Err(InstallError::Unavailable(message))
            if message.contains("restored installed object; failed stage cleanup")
    ));
    audit_outcome(fixture)
}

#[test]
fn recreated_new_control_source_tracks_move_and_restores_old_pair() -> TestResult<()> {
    let outcome = recreated_source(FaultPoint::NewControl)?;

    assert_eq!(outcome.executable, outcome.fixture.old_executable);
    assert_eq!(outcome.control, outcome.fixture.old_control);
    assert!(outcome.stage.join("failed-control").is_dir());
    assert!(outcome.stage.join("executable").is_file());
    assert!(
        fs::symlink_metadata(outcome.stage.join("control"))?
            .file_type()
            .is_socket()
    );
    Ok(())
}

#[test]
fn recreated_new_executable_source_tracks_move_and_restores_old_pair() -> TestResult<()> {
    let outcome = recreated_source(FaultPoint::NewExecutable)?;

    assert_eq!(outcome.executable, outcome.fixture.old_executable);
    assert_eq!(outcome.control, outcome.fixture.old_control);
    assert!(outcome.stage.join("failed-executable").is_file());
    assert!(outcome.stage.join("failed-control").is_dir());
    assert!(
        fs::symlink_metadata(outcome.stage.join("executable"))?
            .file_type()
            .is_socket()
    );
    Ok(())
}

#[test]
fn foreign_executable_is_preserved_with_exact_old_residue() -> TestResult<()> {
    for (point, extra) in [
        (FaultPoint::OldExecutable, None),
        (FaultPoint::NewExecutable, Some("fault-new-executable")),
    ] {
        let outcome = foreign(point)?;

        assert_eq!(
            fs::read(outcome.fixture.root.path().join("tool").join(NAME))?,
            b"foreign"
        );
        assert_eq!(outcome.control, outcome.fixture.old_control);
        assert_eq!(
            fs::symlink_metadata(outcome.stage.join("old-executable"))?.ino(),
            outcome.fixture.old_executable
        );
        if let Some(extra) = extra {
            assert!(outcome.stage.join(extra).is_file(), "missing {extra}");
        }
    }
    Ok(())
}

#[test]
fn foreign_old_control_is_preserved_while_old_executable_restores()
-> Result<(), Box<dyn std::error::Error>> {
    let outcome = foreign(FaultPoint::OldControl)?;

    assert_eq!(outcome.executable, outcome.fixture.old_executable);
    assert_ne!(outcome.control, outcome.fixture.old_control);
    assert_eq!(
        fs::symlink_metadata(outcome.stage.join("old-control"))?.ino(),
        outcome.fixture.old_control
    );
    Ok(())
}

#[test]
fn foreign_new_control_is_preserved_while_old_executable_restores()
-> Result<(), Box<dyn std::error::Error>> {
    let outcome = foreign(FaultPoint::NewControl)?;

    assert_eq!(outcome.executable, outcome.fixture.old_executable);
    assert_ne!(outcome.control, outcome.fixture.old_control);
    assert_eq!(
        fs::symlink_metadata(outcome.stage.join("old-control"))?.ino(),
        outcome.fixture.old_control
    );
    assert!(outcome.stage.join("fault-new-control").is_dir());
    Ok(())
}

#[test]
fn cleanup_failure_keeps_committed_replacement_and_reports_old_residue()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture("cortexfs.object/v2", Some("1.0.0"))?;
    let candidate = candidate(&fixture, "candidate", "2.0.0")?;

    let _fault = ReplaceFault {
        cleanup: true,
        ..ReplaceFault::default()
    }
    .install();
    let result = replace_object(
        fixture.root.path(),
        &candidate,
        InstallTier::System,
        ReplaceMode::Upgrade,
        true,
    );

    assert!(matches!(
        result,
        Err(InstallError::Unavailable(message))
            if message.contains("replacement committed/published, old residue retained")
    ));
    assert_eq!(
        inspect_object(
            fixture.root.path(),
            ObjectClass::Tool,
            NAME,
            InstallTier::System,
        )?
        .object_version(),
        Some("2.0.0")
    );
    assert_eq!(audit_residue(fixture.root.path())?.len(), 1);
    Ok(())
}
