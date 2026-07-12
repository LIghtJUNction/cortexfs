use super::*;
use std::io;

fn fixture() -> Result<(tempfile::TempDir, EntryReceipt, EntryReceipt), InstallError> {
    let root = tempfile::tempdir().map_err(|error| InstallError::unavailable(error.to_string()))?;
    let class_path = root.path().join("tool");
    fs::create_dir_all(&class_path)
        .map_err(|error| InstallError::unavailable(error.to_string()))?;
    let executable_path = class_path.join("example.echo");
    fs::write(&executable_path, b"#!/bin/sh\nprintf ok\n")
        .map_err(|error| InstallError::unavailable(error.to_string()))?;
    fs::set_permissions(&executable_path, fs::Permissions::from_mode(0o755))
        .map_err(|error| InstallError::unavailable(error.to_string()))?;
    let control_path = class_path.join("example.echo.d");
    fs::create_dir_all(&control_path)
        .map_err(|error| InstallError::unavailable(error.to_string()))?;
    let control = crate::support::plain::open_plain_directory(&control_path)
        .map_err(|error| InstallError::unavailable(error.to_string()))?;
    let executable = crate::support::plain::open_plain_file(&executable_path)
        .map_err(|error| InstallError::unavailable(error.to_string()))?;
    let control_receipt = receipt_for(&control, EntryKind::Directory)?;
    let executable_receipt = receipt_for(&executable, EntryKind::Executable)?;
    let digest = Sha256::digest(b"#!/bin/sh\nprintf ok\n").iter().fold(
        String::with_capacity(64),
        |mut output, byte| {
            let _ignored = write!(output, "{byte:02x}");
            output
        },
    );
    write_install_receipt(
        &control,
        &InstallReceiptData {
            class: ObjectClass::Tool,
            name: "example.echo",
            tier: InstallTier::System,
            object_schema: OBJECT_MANIFEST_SCHEMA_V1,
            sha256: &digest,
            control: control_receipt,
            executable: executable_receipt,
        },
    )?;
    Ok((root, control_receipt, executable_receipt))
}

#[test]
fn inspect_verifies_receipt_identity_and_digest() -> Result<(), Box<dyn std::error::Error>> {
    let (root, control, executable) = fixture()?;
    let inspected = inspect_object(
        root.path(),
        ObjectClass::Tool,
        "example.echo",
        InstallTier::System,
    )?;
    assert_eq!(inspected.control_dev(), control.dev);
    assert_eq!(inspected.control_ino(), control.ino);
    assert_eq!(inspected.executable_dev(), executable.dev);
    assert_eq!(inspected.executable_ino(), executable.ino);
    Ok(())
}

#[test]
fn inspect_rejects_unknown_receipt_fields_and_versions() -> Result<(), Box<dyn std::error::Error>> {
    let (root, _control, _executable) = fixture()?;
    let path = root
        .path()
        .join("tool/example.echo.d")
        .join(INSTALL_RECEIPT_FILE);
    let mut receipt: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
    receipt
        .as_object_mut()
        .ok_or("receipt is not an object")?
        .insert("extra".to_owned(), serde_json::Value::Bool(true));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
    fs::write(&path, serde_json::to_vec(&receipt)?)?;
    assert!(
        inspect_object(
            root.path(),
            ObjectClass::Tool,
            "example.echo",
            InstallTier::System,
        )
        .is_err_and(|error| error.message().contains("unknown field"))
    );

    receipt
        .as_object_mut()
        .ok_or("receipt is not an object")?
        .remove("extra");
    let schema = receipt
        .get_mut("schema")
        .ok_or("receipt schema is missing")?;
    *schema = serde_json::Value::String("cortexfs.object-install/v2".to_owned());
    fs::write(&path, serde_json::to_vec(&receipt)?)?;
    assert!(
        inspect_object(
            root.path(),
            ObjectClass::Tool,
            "example.echo",
            InstallTier::System,
        )
        .is_err_and(|error| error.message().contains("unsupported"))
    );
    Ok(())
}

#[test]
fn inspect_detects_executable_replacement_after_open() -> Result<(), Box<dyn std::error::Error>> {
    let (root, _control, _executable) = fixture()?;
    let result = inspect_object_with(
        root.path(),
        ObjectClass::Tool,
        "example.echo",
        InstallTier::System,
        |class| {
            nix::fcntl::renameat2(
                class,
                "example.echo",
                class,
                ".foreign",
                nix::fcntl::RenameFlags::RENAME_NOREPLACE,
            )
            .map_err(|error| InstallError::unavailable(error.to_string()))?;
            let fd = nix::fcntl::openat(
                class,
                "example.echo",
                nix::fcntl::OFlag::O_CREAT
                    | nix::fcntl::OFlag::O_EXCL
                    | nix::fcntl::OFlag::O_WRONLY
                    | nix::fcntl::OFlag::O_NOFOLLOW,
                nix::sys::stat::Mode::from_bits_truncate(0o755),
            )
            .map_err(|error| InstallError::unavailable(error.to_string()))?;
            fs::File::from(fd)
                .write_all(b"foreign")
                .map_err(|error| InstallError::unavailable(error.to_string()))
        },
    );
    assert!(result.is_err_and(|error| error.message().contains("changed")));
    assert_eq!(fs::read(root.path().join("tool/example.echo"))?, b"foreign");
    Ok(())
}

#[test]
fn inspect_detects_control_replacement_after_open() -> Result<(), Box<dyn std::error::Error>> {
    let (root, _control, _executable) = fixture()?;
    let result = inspect_object_with(
        root.path(),
        ObjectClass::Tool,
        "example.echo",
        InstallTier::System,
        |class| {
            nix::fcntl::renameat2(
                class,
                "example.echo.d",
                class,
                ".foreign.d",
                nix::fcntl::RenameFlags::RENAME_NOREPLACE,
            )
            .map_err(|error| InstallError::unavailable(error.to_string()))?;
            nix::sys::stat::mkdirat(
                class,
                "example.echo.d",
                nix::sys::stat::Mode::from_bits_truncate(0o700),
            )
            .map_err(|error| InstallError::unavailable(error.to_string()))
        },
    );
    assert!(result.is_err_and(|error| error.message().contains("changed")));
    assert!(root.path().join("tool/example.echo.d").is_dir());
    Ok(())
}

#[test]
fn inspect_detects_receipt_rewrite_after_open() -> Result<(), Box<dyn std::error::Error>> {
    let (root, _control, _executable) = fixture()?;
    let receipt_path = root
        .path()
        .join("tool/example.echo.d")
        .join(INSTALL_RECEIPT_FILE);
    let result = inspect_object_with(
        root.path(),
        ObjectClass::Tool,
        "example.echo",
        InstallTier::System,
        |_class| {
            fs::set_permissions(&receipt_path, fs::Permissions::from_mode(0o644))
                .map_err(|error| InstallError::unavailable(error.to_string()))?;
            let mut receipt: serde_json::Value = serde_json::from_slice(
                &fs::read(&receipt_path)
                    .map_err(|error| InstallError::unavailable(error.to_string()))?,
            )
            .map_err(|error| InstallError::unavailable(error.to_string()))?;
            let name = receipt
                .get_mut("name")
                .ok_or_else(|| InstallError::unavailable("receipt name is missing"))?;
            *name = serde_json::Value::String("foreign".to_owned());
            fs::write(
                &receipt_path,
                serde_json::to_vec(&receipt)
                    .map_err(|error| InstallError::unavailable(error.to_string()))?,
            )
            .map_err(|error| InstallError::unavailable(error.to_string()))
        },
    );
    assert!(result.is_err_and(|error| error.message().contains("changed")));
    Ok(())
}

#[test]
fn inspect_rejects_in_place_executable_tampering() -> Result<(), Box<dyn std::error::Error>> {
    let (root, _control, _executable) = fixture()?;
    fs::write(root.path().join("tool/example.echo"), b"tampered")?;
    let Err(error) = inspect_object(
        root.path(),
        ObjectClass::Tool,
        "example.echo",
        InstallTier::System,
    ) else {
        return Err(io::Error::other("tampered executable was accepted").into());
    };
    assert!(matches!(error, InstallError::Unavailable(_)));
    assert!(error.message().contains("sha256"));
    Ok(())
}

#[test]
fn inspect_rejects_missing_receipt_without_mutation() -> Result<(), Box<dyn std::error::Error>> {
    let (root, _control, _executable) = fixture()?;
    let receipt = root
        .path()
        .join("tool/example.echo.d")
        .join(INSTALL_RECEIPT_FILE);
    fs::remove_file(&receipt)?;
    let Err(error) = inspect_object(
        root.path(),
        ObjectClass::Tool,
        "example.echo",
        InstallTier::System,
    ) else {
        return Err(io::Error::other("unmanaged object was accepted").into());
    };
    assert!(error.message().contains("receipt"));
    assert!(root.path().join("tool/example.echo").is_file());
    Ok(())
}
