use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt, symlink};

use super::*;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn socket_fixture()
-> std::result::Result<(tempfile::TempDir, SocketReceipt, UnixListener), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o711))?;
    let (receipt, listener) = SocketReceipt::bind(
        root.path(),
        "control.sock",
        (
            nix::unistd::getuid().as_raw(),
            nix::unistd::getgid().as_raw(),
        ),
    )?;
    Ok((root, receipt, listener))
}

fn dir_fixture()
-> std::result::Result<(tempfile::TempDir, EmptyDirReceipt), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let receipt = EmptyDirReceipt::create(
        root.path(),
        "stage",
        nix::unistd::getuid().as_raw(),
        nix::unistd::getgid().as_raw(),
        0o710,
    )?;
    Ok((root, receipt))
}

#[test]
fn empty_dir_receipt_configures_and_cleans_directory() -> TestResult {
    let (_root, receipt) = dir_fixture()?;
    let metadata = fs::symlink_metadata(receipt.path())?;
    assert!(metadata.is_dir());
    assert_eq!(metadata.uid(), nix::unistd::getuid().as_raw());
    assert_eq!(metadata.gid(), nix::unistd::getgid().as_raw());
    assert_eq!(metadata.permissions().mode() & 0o7777, 0o710);
    assert_eq!((metadata.dev(), metadata.ino()), receipt.child);
    receipt.cleanup()?;
    assert!(!receipt.path().exists());
    Ok(())
}

#[test]
fn empty_dir_cleanup_refuses_replacement() -> TestResult {
    let (_root, receipt) = dir_fixture()?;
    fs::remove_dir(receipt.path())?;
    fs::create_dir_all(receipt.path())?;
    let replacement = fs::symlink_metadata(receipt.path())?;
    assert_ne!((replacement.dev(), replacement.ino()), receipt.child);
    assert_eq!(receipt.cleanup(), Err(ReceiptError::CleanupConflict));
    assert!(receipt.path().is_dir());
    Ok(())
}

#[test]
fn empty_dir_cleanup_refuses_symlink_and_non_directory() -> TestResult {
    for symlink_replacement in [false, true] {
        let (_root, receipt) = dir_fixture()?;
        fs::remove_dir(receipt.path())?;
        if symlink_replacement {
            symlink("missing", receipt.path())?;
        } else {
            fs::write(receipt.path(), b"replacement")?;
        }
        assert_eq!(receipt.cleanup(), Err(ReceiptError::CleanupConflict));
        assert!(fs::symlink_metadata(receipt.path()).is_ok());
    }
    Ok(())
}

#[test]
fn empty_dir_cleanup_refuses_nonempty_directory() -> TestResult {
    let (_root, receipt) = dir_fixture()?;
    fs::write(receipt.path().join("keep"), b"data")?;
    assert_eq!(receipt.cleanup(), Err(ReceiptError::CleanupConflict));
    assert_eq!(fs::read(receipt.path().join("keep"))?, b"data");
    Ok(())
}

#[test]
fn empty_dir_cleanup_refuses_rebound_parent() -> TestResult {
    let root = tempfile::tempdir()?;
    let parent = root.path().join("parent");
    let original = root.path().join("original");
    fs::create_dir_all(&parent)?;
    let receipt = EmptyDirReceipt::create(
        &parent,
        "stage",
        nix::unistd::getuid().as_raw(),
        nix::unistd::getgid().as_raw(),
        0o700,
    )?;
    let name = receipt.path().file_name().ok_or("missing name")?.to_owned();
    fs::rename(&parent, &original)?;
    fs::create_dir_all(&parent)?;
    assert_eq!(receipt.cleanup(), Err(ReceiptError::CleanupConflict));
    assert!(original.join(name).is_dir());
    Ok(())
}

#[test]
fn socket_receipt_configures_and_cleans_socket() -> TestResult {
    let (_root, receipt, listener) = socket_fixture()?;
    let metadata = fs::symlink_metadata(receipt.path())?;
    assert!(metadata.file_type().is_socket());
    assert_eq!(metadata.uid(), nix::unistd::getuid().as_raw());
    assert_eq!(metadata.gid(), nix::unistd::getgid().as_raw());
    assert_eq!(metadata.permissions().mode() & 0o7777, 0o600);
    assert_eq!((metadata.dev(), metadata.ino()), receipt.identity());
    drop(listener);
    receipt.cleanup()?;
    assert!(!receipt.path().exists());
    Ok(())
}

#[test]
fn cleanup_refuses_replacement_socket() -> TestResult {
    let (_root, receipt, listener) = socket_fixture()?;
    drop(listener);
    fs::remove_file(receipt.path())?;
    let _replacement = UnixListener::bind(receipt.path())?;
    let replacement = fs::symlink_metadata(receipt.path())?;
    assert_ne!((replacement.dev(), replacement.ino()), receipt.identity());
    assert_eq!(receipt.cleanup(), Err(SocketReceiptError::Cleanup));
    assert!(receipt.path().exists());
    Ok(())
}

#[test]
fn cleanup_refuses_non_socket_and_symlink() -> TestResult {
    for use_symlink in [false, true] {
        let (_root, receipt, listener) = socket_fixture()?;
        drop(listener);
        fs::remove_file(receipt.path())?;
        if use_symlink {
            symlink("missing", receipt.path())?;
        } else {
            fs::write(receipt.path(), b"replacement")?;
        }
        assert_eq!(receipt.cleanup(), Err(SocketReceiptError::Cleanup));
        assert!(fs::symlink_metadata(receipt.path()).is_ok());
    }
    Ok(())
}

#[test]
fn random_hex_has_exact_lowercase_format() -> TestResult {
    let token = random_hex::<32>()?;
    assert_eq!(token.len(), 64);
    assert!(
        token
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );
    Ok(())
}
