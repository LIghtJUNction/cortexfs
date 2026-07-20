use super::{
    ResidueConflict, ResidueEligibility, ResidueError, ResidueKind, apply_cleanup, audit_residue,
    cleanup_residue, prepare_cleanup,
};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::Path;

fn fixture() -> io::Result<tempfile::TempDir> {
    let root = tempfile::tempdir()?;
    for path in ["tool", "agent", "home/1000/tool", "home/1000/agent"] {
        fs::create_dir_all(root.path().join(path))?;
    }
    Ok(root)
}

fn stage_receipt(path: &Path) -> io::Result<(u64, u64)> {
    let metadata = fs::symlink_metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}

#[test]
fn audit_is_sorted_and_does_not_follow_symlinks() -> Result<(), Box<dyn std::error::Error>> {
    let root = fixture()?;
    fs::create_dir_all(root.path().join("tool/.cortexfs-install-z"))?;
    fs::create_dir_all(root.path().join("tool/.cortexfs-cleanup-z"))?;
    fs::create_dir_all(root.path().join("agent/.ctx-rollback-a"))?;
    let outside = tempfile::tempdir()?;
    fs::create_dir_all(outside.path().join(".cortexfs-install-hidden"))?;
    symlink(outside.path(), root.path().join("tool/outside"))?;

    let reports = audit_residue(root.path())?;
    let paths: Vec<&Path> = reports.iter().map(|report| report.path.as_path()).collect();

    assert_eq!(
        paths,
        [
            Path::new("agent/.ctx-rollback-a"),
            Path::new("tool/.cortexfs-cleanup-z"),
            Path::new("tool/.cortexfs-install-z")
        ]
    );
    let cleanup = reports
        .iter()
        .find(|report| report.kind == ResidueKind::Cleanup)
        .ok_or("cleanup quarantine was not audited")?;
    assert_eq!(cleanup.eligibility, ResidueEligibility::AuditOnly);
    Ok(())
}

#[test]
fn cleanup_dry_run_does_not_mutate_stage() -> Result<(), Box<dyn std::error::Error>> {
    let root = fixture()?;
    let stage = root.path().join("tool/.cortexfs-install-dry");
    fs::create_dir_all(&stage)?;
    fs::write(stage.join("entry"), b"data")?;
    let (dev, ino) = stage_receipt(&stage)?;

    let report = cleanup_residue(
        root.path(),
        Path::new("tool/.cortexfs-install-dry"),
        dev,
        ino,
        false,
    )?;

    assert_eq!(report.entries, 2);
    assert!(stage.join("entry").is_file());
    Ok(())
}

#[test]
fn cleanup_exact_receipt_removes_tree_but_preserves_symlink_target()
-> Result<(), Box<dyn std::error::Error>> {
    let root = fixture()?;
    let stage = root.path().join("tool/.cortexfs-install-clean");
    fs::create_dir_all(stage.join("nested/deeper"))?;
    fs::write(stage.join("nested/deeper/file"), b"data")?;
    let target = root.path().join("target");
    fs::write(&target, b"keep")?;
    symlink(&target, stage.join("nested/link"))?;
    let (dev, ino) = stage_receipt(&stage)?;

    let report = cleanup_residue(
        root.path(),
        Path::new("tool/.cortexfs-install-clean"),
        dev,
        ino,
        true,
    )?;

    assert_eq!(report.entries, 5);
    assert!(!stage.exists());
    assert_eq!(fs::read(&target)?, b"keep");
    Ok(())
}

#[test]
fn audit_and_cleanup_accept_non_utf8_descendants() -> Result<(), Box<dyn std::error::Error>> {
    let root = fixture()?;
    let relative = Path::new("tool/.cortexfs-install-non-utf8");
    let stage = root.path().join(relative);
    let nested = stage.join(OsStr::from_bytes(b"nested-\xff"));
    fs::create_dir_all(&nested)?;
    fs::write(nested.join(OsStr::from_bytes(b"file-\xfe")), b"data")?;
    let (dev, ino) = stage_receipt(&stage)?;

    let reports = audit_residue(root.path())?;
    assert!(reports.iter().any(|report| report.path == relative));
    let report = cleanup_residue(root.path(), relative, dev, ino, true)?;

    assert_eq!(report.entries, 3);
    assert!(!stage.exists());
    Ok(())
}

#[test]
fn cleanup_wrong_receipt_preserves_tree() -> Result<(), Box<dyn std::error::Error>> {
    let root = fixture()?;
    let stage = root.path().join("agent/.cortexfs-install-wrong");
    fs::create_dir_all(&stage)?;
    let (dev, ino) = stage_receipt(&stage)?;

    let result = cleanup_residue(
        root.path(),
        Path::new("agent/.cortexfs-install-wrong"),
        dev,
        ino.saturating_add(1),
        true,
    );

    assert!(matches!(result, Err(ResidueError::Conflict(_))));
    assert!(stage.is_dir());
    Ok(())
}

#[test]
fn cleanup_plan_replacement_preserves_replacement() -> Result<(), Box<dyn std::error::Error>> {
    let root = fixture()?;
    let relative = Path::new("tool/.cortexfs-install-race");
    let stage = root.path().join(relative);
    fs::create_dir_all(&stage)?;
    fs::write(stage.join("owned"), b"owned")?;
    let (dev, ino) = stage_receipt(&stage)?;

    let prepared = prepare_cleanup(root.path(), relative, dev, ino, true)?;
    fs::rename(&stage, root.path().join("tool/.captured-stage"))?;
    fs::create_dir_all(&stage)?;

    let result = apply_cleanup(&prepared);

    assert!(matches!(
        result,
        Err(ResidueError::Conflict(ResidueConflict {
            quarantine: None,
            ..
        }))
    ));
    assert!(stage.is_dir());
    assert!(root.path().join("tool/.captured-stage/owned").is_file());
    Ok(())
}

#[test]
fn cleanup_leaf_replacement_restores_the_install_stage() -> Result<(), Box<dyn std::error::Error>> {
    let root = fixture()?;
    let relative = Path::new("tool/.cortexfs-install-leaf-race");
    let stage = root.path().join(relative);
    fs::create_dir_all(&stage)?;
    fs::write(stage.join("owned"), b"owned")?;
    let (dev, ino) = stage_receipt(&stage)?;
    let prepared = prepare_cleanup(root.path(), relative, dev, ino, true)?;
    fs::rename(stage.join("owned"), stage.join(".captured-owned"))?;
    fs::write(stage.join("owned"), b"replacement")?;

    let result = apply_cleanup(&prepared);
    let Err(ResidueError::Conflict(conflict)) = result else {
        return Err("leaf replacement did not fail closed".into());
    };
    assert_eq!(conflict.quarantine, None);
    assert!(
        conflict
            .detail
            .contains("restored original install-stage name")
    );
    assert_eq!(fs::read(stage.join("owned"))?, b"replacement");
    assert_eq!(fs::read(stage.join(".captured-owned"))?, b"owned");
    assert!(fs::read_dir(&stage)?.all(|entry| {
        entry.is_ok_and(|entry| {
            !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".cortexfs-cleanup-entry-")
        })
    }));
    Ok(())
}

#[test]
fn rollback_cleanup_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let root = fixture()?;
    for relative in [
        Path::new("agent/.ctx-rollback-test"),
        Path::new("agent/.cortexfs-cleanup-test"),
    ] {
        let residue = root.path().join(relative);
        fs::create_dir_all(&residue)?;
        let (dev, ino) = stage_receipt(&residue)?;

        let result = cleanup_residue(root.path(), relative, dev, ino, false);

        assert!(
            matches!(result, Err(ResidueError::Invalid(message)) if message.contains("audit-only"))
        );
        assert!(residue.is_dir());
    }
    Ok(())
}
