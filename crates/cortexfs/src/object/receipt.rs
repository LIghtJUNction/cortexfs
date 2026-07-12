use super::install::{InstallError, InstallTier, OBJECT_MANIFEST_SCHEMA_V1, install_class_path};
use crate::support::plain::{read_file_to_string, write_text_file_at};
use crate::{ObjectClass, is_object_name};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::io::{Read, Seek, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

const INSTALL_RECEIPT_FILE: &str = ".cortexfs-receipt.json";
const INSTALL_RECEIPT_SCHEMA_V1: &str = "cortexfs.object-install/v1";
const MAX_INSTALL_RECEIPT_BYTES: u64 = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EntryKind {
    Directory,
    File,
    Executable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EntryReceipt {
    pub(crate) dev: u64,
    pub(crate) ino: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileSnapshot {
    len: u64,
    mode: u32,
    mtime: i64,
    mtime_nsec: i64,
    ctime: i64,
    ctime_nsec: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ExecutableReceipt {
    dev: u64,
    ino: u64,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct InstallReceipt {
    schema: String,
    object_schema: String,
    class: String,
    name: String,
    tier: String,
    executable: ExecutableReceipt,
    control: EntryReceipt,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct InstallReceiptData<'a> {
    pub(crate) class: ObjectClass,
    pub(crate) name: &'a str,
    pub(crate) tier: InstallTier,
    pub(crate) object_schema: &'a str,
    pub(crate) sha256: &'a str,
    pub(crate) control: EntryReceipt,
    pub(crate) executable: EntryReceipt,
}

/// One installed object whose lifecycle receipt, inode pair, executable mode,
/// and executable digest were verified through retained no-follow descriptors.
#[derive(Debug)]
pub struct InspectedObject {
    class: ObjectClass,
    name: String,
    tier: InstallTier,
    receipt: InstallReceipt,
    pub(crate) class_fd: fs::File,
    _control: fs::File,
    _receipt: fs::File,
    _executable: fs::File,
}

impl InspectedObject {
    /// Returns the verified object class.
    #[must_use]
    pub const fn class(&self) -> ObjectClass {
        self.class
    }

    /// Returns the verified object name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the verified installation tier.
    #[must_use]
    pub const fn tier(&self) -> InstallTier {
        self.tier
    }

    /// Returns the manifest schema recorded at installation.
    #[must_use]
    pub fn object_schema(&self) -> &str {
        &self.receipt.object_schema
    }

    /// Returns the verified executable SHA-256.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.receipt.executable.sha256
    }

    /// Returns the verified executable device number.
    #[must_use]
    pub const fn executable_dev(&self) -> u64 {
        self.receipt.executable.dev
    }

    /// Returns the verified executable inode number.
    #[must_use]
    pub const fn executable_ino(&self) -> u64 {
        self.receipt.executable.ino
    }

    /// Returns the verified control-directory device number.
    #[must_use]
    pub const fn control_dev(&self) -> u64 {
        self.receipt.control.dev
    }

    /// Returns the verified control-directory inode number.
    #[must_use]
    pub const fn control_ino(&self) -> u64 {
        self.receipt.control.ino
    }
}

pub(crate) fn write_install_receipt(
    control: &fs::File,
    data: &InstallReceiptData<'_>,
) -> Result<(), InstallError> {
    let receipt = InstallReceipt {
        schema: INSTALL_RECEIPT_SCHEMA_V1.to_owned(),
        object_schema: data.object_schema.to_owned(),
        class: data.class.as_str().to_owned(),
        name: data.name.to_owned(),
        tier: data.tier.as_str().to_owned(),
        executable: ExecutableReceipt {
            dev: data.executable.dev,
            ino: data.executable.ino,
            sha256: data.sha256.to_ascii_lowercase(),
        },
        control: data.control,
    };
    let mut content = serde_json::to_string(&receipt).map_err(|error| {
        InstallError::unavailable(format!("cannot encode object install receipt: {error}"))
    })?;
    content.push('\n');
    write_text_file_at(control, INSTALL_RECEIPT_FILE, &content, 0o444).map_err(|error| {
        InstallError::unavailable(format!("cannot write object install receipt: {error}"))
    })
}

/// Inspects one exact installer-managed object without modifying its backing tree.
pub fn inspect_object(
    root: &Path,
    class: ObjectClass,
    name: &str,
    tier: InstallTier,
) -> Result<InspectedObject, InstallError> {
    inspect_object_with(root, class, name, tier, |_class| Ok(()))
}

fn inspect_object_with(
    root: &Path,
    class: ObjectClass,
    name: &str,
    tier: InstallTier,
    after_open: impl FnOnce(&fs::File) -> Result<(), InstallError>,
) -> Result<InspectedObject, InstallError> {
    if !matches!(class, ObjectClass::Tool | ObjectClass::Agent) || !is_object_name(name) {
        return Err(InstallError::invalid("invalid installed object identity"));
    }
    let class_path = install_class_path(root, class, tier)?;
    let class_fd = crate::support::plain::open_plain_directory(&class_path).map_err(|error| {
        InstallError::unavailable(format!("cannot open installed object class: {error}"))
    })?;
    let control_name = format!("{name}.d");
    let control_fd = open_directory(&class_fd, &control_name)?;
    let mut receipt_fd = open_receipt(&control_fd)?;
    let receipt_entry = receipt_for(&receipt_fd, EntryKind::File)?;
    let receipt = read_install_receipt(&mut receipt_fd)?;
    validate_receipt(&receipt, class, name, tier)?;
    let executable_fd = open_executable(&class_fd, name)?;
    let control_receipt = receipt_for(&control_fd, EntryKind::Directory)?;
    let executable_receipt = receipt_for(&executable_fd, EntryKind::Executable)?;
    let executable_snapshot = file_snapshot(&executable_fd)?;
    if control_receipt != receipt.control
        || executable_receipt.dev != receipt.executable.dev
        || executable_receipt.ino != receipt.executable.ino
    {
        return Err(InstallError::unavailable(
            "installed object receipt does not match retained entries",
        ));
    }
    after_open(&class_fd)?;
    if !entry_matches(
        &class_fd,
        &control_name,
        receipt.control,
        EntryKind::Directory,
    ) || !entry_matches(&class_fd, name, executable_receipt, EntryKind::Executable)
    {
        return Err(InstallError::unavailable(
            "installed object changed during inspection",
        ));
    }
    let mut executable_fd = executable_fd;
    verify_executable(&mut executable_fd, &receipt.executable.sha256, None).map_err(|error| {
        InstallError::unavailable(format!(
            "cannot verify installed object executable: {}",
            error.message()
        ))
    })?;
    let receipt_after = read_install_receipt(&mut receipt_fd).map_err(|error| {
        InstallError::unavailable(format!(
            "object install receipt changed during inspection: {}",
            error.message()
        ))
    })?;
    if receipt_after != receipt || file_snapshot(&executable_fd)? != executable_snapshot {
        return Err(InstallError::unavailable(
            "installed object changed during inspection",
        ));
    }
    if !entry_matches(
        &class_fd,
        &control_name,
        receipt.control,
        EntryKind::Directory,
    ) || !entry_matches(&class_fd, name, executable_receipt, EntryKind::Executable)
        || !entry_matches(
            &control_fd,
            INSTALL_RECEIPT_FILE,
            receipt_entry,
            EntryKind::File,
        )
    {
        return Err(InstallError::unavailable(
            "installed object changed during inspection",
        ));
    }
    Ok(InspectedObject {
        class,
        name: name.to_owned(),
        tier,
        receipt,
        class_fd,
        _control: control_fd,
        _receipt: receipt_fd,
        _executable: executable_fd,
    })
}

fn read_install_receipt(file: &mut fs::File) -> Result<InstallReceipt, InstallError> {
    let metadata = file.metadata().map_err(|error| {
        InstallError::unavailable(format!("cannot inspect object install receipt: {error}"))
    })?;
    if !metadata.is_file() || metadata.len() > MAX_INSTALL_RECEIPT_BYTES {
        return Err(InstallError::unavailable(
            "object install receipt must be a bounded regular file",
        ));
    }
    file.rewind().map_err(|error| {
        InstallError::unavailable(format!("cannot rewind object install receipt: {error}"))
    })?;
    let content = read_file_to_string(
        file.try_clone().map_err(|error| {
            InstallError::unavailable(format!("cannot retain object install receipt: {error}"))
        })?,
        metadata.len(),
    )
    .map_err(|error| {
        InstallError::unavailable(format!("cannot read object install receipt: {error}"))
    })?;
    serde_json::from_str(&content).map_err(|error| {
        InstallError::unavailable(format!("invalid object install receipt: {error}"))
    })
}

fn validate_receipt(
    receipt: &InstallReceipt,
    class: ObjectClass,
    name: &str,
    tier: InstallTier,
) -> Result<(), InstallError> {
    if receipt.schema != INSTALL_RECEIPT_SCHEMA_V1 {
        return Err(InstallError::unavailable(format!(
            "unsupported object install receipt schema: {}",
            receipt.schema
        )));
    }
    if receipt.object_schema != OBJECT_MANIFEST_SCHEMA_V1 {
        return Err(InstallError::unavailable(format!(
            "unsupported installed object schema: {}",
            receipt.object_schema
        )));
    }
    if receipt.class != class.as_str() || receipt.name != name || receipt.tier != tier.as_str() {
        return Err(InstallError::unavailable(
            "object install receipt identity mismatch",
        ));
    }
    if receipt.executable.sha256.len() != 64
        || !receipt
            .executable
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(InstallError::unavailable(
            "object install receipt has invalid sha256",
        ));
    }
    Ok(())
}

fn open_directory(parent: &fs::File, name: &str) -> Result<fs::File, InstallError> {
    nix::fcntl::openat(
        parent,
        name,
        nix::fcntl::OFlag::O_DIRECTORY
            | nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map(fs::File::from)
    .map_err(|error| {
        InstallError::unavailable(format!("cannot open installed object controls: {error}"))
    })
}

fn open_executable(parent: &fs::File, name: &str) -> Result<fs::File, InstallError> {
    nix::fcntl::openat(
        parent,
        name,
        nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_NONBLOCK
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map(fs::File::from)
    .map_err(|error| {
        InstallError::unavailable(format!("cannot open installed object executable: {error}"))
    })
}

fn open_receipt(control: &fs::File) -> Result<fs::File, InstallError> {
    nix::fcntl::openat(
        control,
        INSTALL_RECEIPT_FILE,
        nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_NONBLOCK
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map(fs::File::from)
    .map_err(|error| {
        InstallError::unavailable(format!("cannot open object install receipt: {error}"))
    })
}

fn file_snapshot(file: &fs::File) -> Result<FileSnapshot, InstallError> {
    let metadata = file.metadata().map_err(|error| {
        InstallError::unavailable(format!("cannot inspect retained executable: {error}"))
    })?;
    Ok(FileSnapshot {
        len: metadata.len(),
        mode: metadata.mode(),
        mtime: metadata.mtime(),
        mtime_nsec: metadata.mtime_nsec(),
        ctime: metadata.ctime(),
        ctime_nsec: metadata.ctime_nsec(),
    })
}

pub(crate) fn receipt_for(file: &fs::File, kind: EntryKind) -> Result<EntryReceipt, InstallError> {
    let metadata = file.metadata().map_err(|error| {
        InstallError::unavailable(format!("cannot receipt object entry: {error}"))
    })?;
    let valid = match kind {
        EntryKind::Directory => metadata.is_dir(),
        EntryKind::File => metadata.is_file(),
        EntryKind::Executable => metadata.is_file() && metadata.permissions().mode() & 0o111 != 0,
    };
    if !valid || metadata.file_type().is_symlink() {
        return Err(InstallError::unavailable("object entry has wrong type"));
    }
    Ok(EntryReceipt {
        dev: metadata.dev(),
        ino: metadata.ino(),
    })
}

pub(crate) fn entry_matches(
    directory: &fs::File,
    name: &str,
    receipt: EntryReceipt,
    kind: EntryKind,
) -> bool {
    nix::sys::stat::fstatat(directory, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW).is_ok_and(
        |stat| {
            let file_kind = nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode);
            let kind_matches = match kind {
                EntryKind::Directory => file_kind.contains(nix::sys::stat::SFlag::S_IFDIR),
                EntryKind::File => file_kind.contains(nix::sys::stat::SFlag::S_IFREG),
                EntryKind::Executable => {
                    file_kind.contains(nix::sys::stat::SFlag::S_IFREG) && stat.st_mode & 0o111 != 0
                }
            };
            kind_matches && (stat.st_dev, stat.st_ino) == (receipt.dev, receipt.ino)
        },
    )
}

pub(crate) fn verify_executable(
    source: &mut fs::File,
    expected: &str,
    mut target: Option<&mut fs::File>,
) -> Result<(), InstallError> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = source
            .read(&mut buffer)
            .map_err(|error| InstallError::invalid(format!("cannot read executable: {error}")))?;
        if count == 0 {
            break;
        }
        let chunk = buffer
            .get(..count)
            .ok_or_else(|| InstallError::invalid("invalid executable read size"))?;
        hasher.update(chunk);
        if let Some(target) = target.as_mut() {
            target.write_all(chunk).map_err(|error| {
                InstallError::unavailable(format!("cannot stage executable: {error}"))
            })?;
        }
    }
    let actual = hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            let _ignored = write!(output, "{byte:02x}");
            output
        });
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(InstallError::invalid(format!(
            "executable sha256 mismatch: expected {expected}, got {actual}"
        )));
    }
    source
        .rewind()
        .map_err(|error| InstallError::invalid(format!("cannot rewind executable: {error}")))
}

#[cfg(test)]
mod tests;
