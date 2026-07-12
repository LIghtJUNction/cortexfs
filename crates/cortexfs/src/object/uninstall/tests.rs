use super::{restore_exact, uninstall_object, uninstall_with};
use crate::ObjectClass;
use crate::object::install::{InstallTier, install_object};
use crate::object::receipt::{EntryKind, EntryReceipt, inspect_object};
use crate::object::residue::{ResidueOccupancy, audit_residue};
use crate::support::plain::open_plain_directory;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};

struct Fixture {
    root: tempfile::TempDir,
    bytes: Vec<u8>,
}

fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::create_dir_all(root.path().join("tool"))?;
    let artifact = root.path().join("artifact");
    let bytes = b"#!/bin/sh\nprintf ok\n".to_vec();
    fs::write(&artifact, &bytes)?;
    fs::set_permissions(&artifact, fs::Permissions::from_mode(0o755))?;
    let digest =
        Sha256::digest(&bytes)
            .iter()
            .fold(String::with_capacity(64), |mut output, byte| {
                let _ignored = write!(output, "{byte:02x}");
                output
            });
    let manifest = root.path().join("tool.json");
    fs::write(
        &manifest,
        serde_json::to_vec(&json!({
            "schema": "cortexfs.object/v1",
            "class": "tool",
            "name": "example.echo",
            "executable": { "path": artifact, "sha256": digest },
            "controls": {
                "description": "echo",
                "schema": r#"{"type":"object"}"#,
                "cap": "text",
                "policy": "allow example_t tool:example.echo execute"
            }
        }))?,
    )?;
    install_object(root.path(), &manifest, InstallTier::System)?;
    Ok(Fixture { root, bytes })
}

fn occupied_stage(root: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    audit_residue(root)?
        .into_iter()
        .find(|report| report.occupancy == ResidueOccupancy::Occupied)
        .map(|report| report.path)
        .ok_or_else(|| "occupied uninstall stage is missing".into())
}

#[test]
fn dry_run_is_read_only_and_apply_removes_exact_object() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture()?;
    let executable = fixture.root.path().join("tool/example.echo");
    let control = fixture.root.path().join("tool/example.echo.d");
    let before = (fs::read(&executable)?, fs::read(control.join("policy"))?);
    let residues_before = audit_residue(fixture.root.path())?.len();

    let inspected = uninstall_object(
        fixture.root.path(),
        ObjectClass::Tool,
        "example.echo",
        InstallTier::System,
        false,
    )?;
    assert_eq!(inspected.name(), "example.echo");
    assert_eq!(
        (fs::read(&executable)?, fs::read(control.join("policy"))?),
        before
    );
    assert_eq!(audit_residue(fixture.root.path())?.len(), residues_before);

    uninstall_object(
        fixture.root.path(),
        ObjectClass::Tool,
        "example.echo",
        InstallTier::System,
        true,
    )?;
    assert!(!executable.exists());
    assert!(!control.exists());
    assert_eq!(audit_residue(fixture.root.path())?.len(), residues_before);
    assert!(
        inspect_object(
            fixture.root.path(),
            ObjectClass::Tool,
            "example.echo",
            InstallTier::System,
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn symlink_source_is_rejected_before_mutation() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture()?;
    let links = tempfile::tempdir()?;
    let source = links.path().join("source");
    symlink(fixture.root.path(), &source)?;
    let executable = fixture.root.path().join("tool/example.echo");
    let control = fixture.root.path().join("tool/example.echo.d");
    let before = (fs::read(&executable)?, fs::read(control.join("policy"))?);
    let residues_before = audit_residue(fixture.root.path())?.len();
    for apply in [false, true] {
        let error = uninstall_object(
            &source,
            ObjectClass::Tool,
            "example.echo",
            InstallTier::System,
            apply,
        )
        .err()
        .ok_or("symlink source was accepted")?;
        assert!(error.message().contains("cannot open durable source"));
        assert_eq!(
            (fs::read(&executable)?, fs::read(control.join("policy"))?),
            before
        );
        assert_eq!(audit_residue(fixture.root.path())?.len(), residues_before);
    }
    Ok(())
}

#[test]
fn restore_exact_rejects_a_foreign_quarantine_receipt() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir_all(&source)?;
    fs::create_dir_all(&target)?;
    let expected = source.join("expected");
    fs::write(&expected, b"expected")?;
    fs::set_permissions(&expected, fs::Permissions::from_mode(0o755))?;
    let metadata = expected.metadata()?;
    let receipt = EntryReceipt {
        dev: metadata.dev(),
        ino: metadata.ino(),
    };
    let foreign = target.join("executable");
    fs::write(&foreign, b"foreign")?;
    fs::set_permissions(&foreign, fs::Permissions::from_mode(0o755))?;

    let source_fd = open_plain_directory(&source)?;
    let target_fd = open_plain_directory(&target)?;
    let Err(error) = restore_exact(
        &target_fd,
        "executable",
        &source_fd,
        "executable",
        receipt,
        EntryKind::Executable,
    ) else {
        return Err("foreign quarantine receipt was restored".into());
    };

    assert!(error.contains("receipt changed"));
    assert_eq!(fs::read(foreign)?, b"foreign");
    assert!(!source.join("executable").exists());
    Ok(())
}

#[test]
fn unsupported_control_entry_retains_complete_quarantine() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = fixture()?;
    nix::unistd::mkfifo(
        &fixture.root.path().join("tool/example.echo.d/foreign-fifo"),
        nix::sys::stat::Mode::from_bits_truncate(0o600),
    )?;
    let error = uninstall_object(
        fixture.root.path(),
        ObjectClass::Tool,
        "example.echo",
        InstallTier::System,
        true,
    )
    .err()
    .ok_or("unsupported control entry was deleted")?;
    assert!(error.message().contains("uninstall cleanup failed"));
    assert!(!fixture.root.path().join("tool/example.echo").exists());
    assert!(!fixture.root.path().join("tool/example.echo.d").exists());
    let stage = occupied_stage(fixture.root.path())?;
    assert!(
        fixture
            .root
            .path()
            .join(stage)
            .join("control/foreign-fifo")
            .symlink_metadata()?
            .file_type()
            .is_fifo()
    );
    Ok(())
}

#[test]
fn executable_recreation_is_preserved_and_original_is_retained()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture()?;
    let error = uninstall_with(
        fixture.root.path(),
        ObjectClass::Tool,
        "example.echo",
        InstallTier::System,
        true,
        1,
    )
    .err()
    .ok_or("foreign executable was not detected")?;
    assert!(error.message().contains("retained residue"));
    assert_eq!(
        fs::read(fixture.root.path().join("tool/example.echo"))?,
        b"foreign"
    );
    assert!(fixture.root.path().join("tool/example.echo.d").is_dir());
    let stage = occupied_stage(fixture.root.path())?;
    assert_eq!(
        fs::read(fixture.root.path().join(stage).join("executable"))?,
        fixture.bytes
    );
    Ok(())
}

#[test]
fn control_recreation_is_preserved_and_original_pair_is_retained()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture()?;
    let error = uninstall_with(
        fixture.root.path(),
        ObjectClass::Tool,
        "example.echo",
        InstallTier::System,
        true,
        2,
    )
    .err()
    .ok_or("foreign control was not detected")?;
    assert!(error.message().contains("retained residue"));
    assert!(!fixture.root.path().join("tool/example.echo").exists());
    assert!(fixture.root.path().join("tool/example.echo.d").is_dir());
    let stage = occupied_stage(fixture.root.path())?;
    assert_eq!(
        fs::read(fixture.root.path().join(&stage).join("executable"))?,
        fixture.bytes
    );
    assert!(fixture.root.path().join(stage).join("control").is_dir());
    Ok(())
}
