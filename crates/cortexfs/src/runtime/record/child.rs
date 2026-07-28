use super::*;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::sync::atomic::{AtomicU64, Ordering};

static CHILD_STAGE_ID: AtomicU64 = AtomicU64::new(0);
const CHILD_RECEIPT_FILE: &str = ".receipt";
const CHILD_RECEIPT_BYTES: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChildHandoffStage {
    Staging,
    Artifact,
    Agent,
    Session,
    Status,
    Handoff,
    Result,
    Refs,
    Publish,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChildClaimStage {
    Staging,
    Publish,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChildFinishStage {
    BeforeResultPublish,
    AfterResultRecheck,
    AfterResultExchange,
    BeforeResultCleanup,
    AfterResultPublish,
    BeforeRefsPublish,
    AfterRefsRecheck,
    AfterRefsExchange,
    BeforeRefsCleanup,
    AfterRefsPublish,
    BeforeStatus,
    AfterStatusRecheck,
    AfterStatusExchange,
    BeforeStatusCleanup,
}

#[derive(Clone, Copy)]
enum ReplacePoint {
    Prepared,
    Rechecked,
    Exchanged,
    Quarantined,
}

fn same_file(stat: &libc::stat, receipt: &libc::stat) -> bool {
    stat.st_mode & libc::S_IFMT == libc::S_IFREG
        && (stat.st_dev, stat.st_ino) == (receipt.st_dev, receipt.st_ino)
}

fn read_child_receipt_guard(child: &fs::File) -> Result<Option<String>, ChildContextRecordError> {
    let max_bytes = u64::try_from(CHILD_RECEIPT_BYTES * 2 + 1)
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let before = match nix::sys::stat::fstatat(
        child,
        CHILD_RECEIPT_FILE,
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    ) {
        Ok(stat) => stat,
        Err(nix::errno::Errno::ENOENT) => return Ok(None),
        Err(_) => return Err(ChildContextRecordError::CannotRecord),
    };
    let value = support::plain::read_small_text_file_at(
        child,
        CHILD_RECEIPT_FILE,
        max_bytes,
        "invalid child receipt",
    )
    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let after = nix::sys::stat::fstatat(
        child,
        CHILD_RECEIPT_FILE,
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let Some(value) = value.strip_suffix('\n') else {
        return Err(ChildContextRecordError::CannotRecord);
    };
    if !same_file(&before, &after)
        || value.len() != CHILD_RECEIPT_BYTES * 2
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(ChildContextRecordError::CannotRecord);
    }
    Ok(Some(value.to_owned()))
}

fn verify_child_receipt_guard(
    child: &fs::File,
    receipt: &ChildHandoffReceipt,
) -> Result<(), ChildContextRecordError> {
    if let Some(expected) = receipt.guard.as_deref()
        && read_child_receipt_guard(child)?.as_deref() != Some(expected)
    {
        return Err(ChildContextRecordError::CannotRecord);
    }
    Ok(())
}

pub(crate) fn is_plain_channel_directory(stat: &libc::stat) -> bool {
    stat.st_mode & libc::S_IFMT == libc::S_IFDIR
}

fn quarantine_unlink(
    directory: &fs::File,
    name: &str,
    receipt: &libc::stat,
    mut quarantined: impl FnMut() -> Result<(), ChildContextRecordError>,
) -> Result<(), ChildContextRecordError> {
    let quarantine = format!(
        ".finish-quarantine-{}-{}",
        std::process::id(),
        CHILD_STAGE_ID.fetch_add(1, Ordering::Relaxed)
    );
    nix::fcntl::renameat2(
        directory,
        name,
        directory,
        quarantine.as_str(),
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let isolated = nix::sys::stat::fstatat(
        directory,
        quarantine.as_str(),
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    if !same_file(&isolated, receipt) {
        let original_absent =
            nix::sys::stat::fstatat(directory, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
                .is_err_and(|error| error == nix::errno::Errno::ENOENT);
        if original_absent {
            nix::fcntl::renameat2(
                directory,
                quarantine.as_str(),
                directory,
                name,
                nix::fcntl::RenameFlags::RENAME_NOREPLACE,
            )
            .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        }
        return Err(ChildContextRecordError::CannotRecord);
    }
    quarantined()?;
    let isolated = nix::sys::stat::fstatat(
        directory,
        quarantine.as_str(),
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    if !same_file(&isolated, receipt) {
        return Err(ChildContextRecordError::CannotRecord);
    }
    nix::unistd::unlinkat(
        directory,
        quarantine.as_str(),
        nix::unistd::UnlinkatFlags::NoRemoveDir,
    )
    .map_err(|_error| ChildContextRecordError::CannotRecord)
}

#[expect(
    clippy::too_many_lines,
    reason = "the exchange, verified rollback, and quarantine cleanup form one CAS transaction"
)]
fn replace_child_file(
    directory: &fs::File,
    target: &str,
    temporary: &str,
    value: &str,
    mut hook: impl FnMut(ReplacePoint) -> Result<(), ChildContextRecordError>,
) -> Result<(), ChildContextRecordError> {
    let original =
        nix::sys::stat::fstatat(directory, target, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
            .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    if original.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(ChildContextRecordError::CannotRecord);
    }
    let created = nix::fcntl::openat(
        directory,
        temporary,
        nix::fcntl::OFlag::O_WRONLY
            | nix::fcntl::OFlag::O_CREAT
            | nix::fcntl::OFlag::O_EXCL
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::from_bits_truncate(0o600),
    )
    .map(fs::File::from)
    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let publish = (|| {
        nix::unistd::fchown(
            &created,
            Some(nix::unistd::Uid::from_raw(original.st_uid)),
            Some(nix::unistd::Gid::from_raw(original.st_gid)),
        )
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        nix::sys::stat::fchmod(
            &created,
            nix::sys::stat::Mode::from_bits_truncate(original.st_mode & 0o7777),
        )
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        (&created)
            .write_all(value.as_bytes())
            .and_then(|()| created.sync_all())
            .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        let staged = nix::sys::stat::fstat(&created)
            .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        hook(ReplacePoint::Prepared)?;
        let current =
            nix::sys::stat::fstatat(directory, target, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
                .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        if current.st_mode & libc::S_IFMT != libc::S_IFREG
            || (current.st_dev, current.st_ino) != (original.st_dev, original.st_ino)
        {
            return Err(ChildContextRecordError::CannotRecord);
        }
        hook(ReplacePoint::Rechecked)?;
        let observed =
            nix::sys::stat::fstatat(directory, target, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
                .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        nix::fcntl::renameat2(
            directory,
            temporary,
            directory,
            target,
            nix::fcntl::RenameFlags::RENAME_EXCHANGE,
        )
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        hook(ReplacePoint::Exchanged)?;
        let published =
            nix::sys::stat::fstatat(directory, target, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
                .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        let displaced = nix::sys::stat::fstatat(
            directory,
            temporary,
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        if !same_file(&published, &staged) || !same_file(&displaced, &original) {
            if same_file(&published, &staged) && same_file(&displaced, &observed) {
                nix::fcntl::renameat2(
                    directory,
                    temporary,
                    directory,
                    target,
                    nix::fcntl::RenameFlags::RENAME_EXCHANGE,
                )
                .map_err(|_error| ChildContextRecordError::CannotRecord)?;
                let restored = nix::sys::stat::fstatat(
                    directory,
                    target,
                    nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
                )
                .map_err(|_error| ChildContextRecordError::CannotRecord)?;
                let staged_name = nix::sys::stat::fstatat(
                    directory,
                    temporary,
                    nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
                )
                .map_err(|_error| ChildContextRecordError::CannotRecord)?;
                if !same_file(&restored, &observed) || !same_file(&staged_name, &staged) {
                    return Err(ChildContextRecordError::CannotRecord);
                }
                quarantine_unlink(directory, temporary, &staged, || {
                    hook(ReplacePoint::Quarantined)
                })?;
            }
            return Err(ChildContextRecordError::CannotRecord);
        }
        quarantine_unlink(directory, temporary, &original, || {
            hook(ReplacePoint::Quarantined)
        })?;
        directory
            .sync_all()
            .map_err(|_error| ChildContextRecordError::CannotRecord)
    })();
    if publish.is_err() {
        let staged = nix::sys::stat::fstat(&created);
        if let Ok(staged) = staged
            && nix::sys::stat::fstatat(
                directory,
                temporary,
                nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
            )
            .as_ref()
            .is_ok_and(|current| same_file(current, &staged))
        {
            quarantine_unlink(directory, temporary, &staged, || Ok(()))?;
        }
    }
    publish
}

struct ChildPrivilegeLease<'a> {
    directory: &'a fs::File,
    parent: &'a fs::File,
    name: &'a str,
    original: libc::stat,
    restored: bool,
}

impl<'a> ChildPrivilegeLease<'a> {
    fn acquire(
        directory: &'a fs::File,
        parent: &'a fs::File,
        name: &'a str,
        receipt: &ChildHandoffReceipt,
    ) -> Result<Self, ChildContextRecordError> {
        let original = nix::sys::stat::fstat(directory)
            .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        if !is_plain_channel_directory(&original) {
            return Err(ChildContextRecordError::CannotRecord);
        }
        let effective_uid = nix::unistd::Uid::effective();
        if !effective_uid.is_root() || effective_uid.as_raw() == original.st_uid {
            return Err(ChildContextRecordError::CannotRecord);
        }
        let mut lease = Self {
            directory,
            parent,
            name,
            original,
            restored: false,
        };
        let acquired = (|| {
            nix::unistd::fchown(
                directory,
                Some(effective_uid),
                Some(nix::unistd::Gid::effective()),
            )
            .map_err(|_error| ChildContextRecordError::CannotRecord)?;
            nix::sys::stat::fchmod(directory, nix::sys::stat::Mode::from_bits_truncate(0o700))
                .map_err(|_error| ChildContextRecordError::CannotRecord)?;
            let leased = nix::sys::stat::fstat(directory)
                .map_err(|_error| ChildContextRecordError::CannotRecord)?;
            let path =
                nix::sys::stat::fstatat(parent, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
                    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
            if (leased.st_dev, leased.st_ino) != (original.st_dev, original.st_ino)
                || (path.st_dev, path.st_ino) != (receipt.dev, receipt.ino)
            {
                return Err(ChildContextRecordError::CannotRecord);
            }
            Ok(())
        })();
        if let Err(error) = acquired {
            return match lease.restore(Err(error)) {
                Err(error) => Err(error),
                Ok(()) => Err(ChildContextRecordError::CannotRecord),
            };
        }
        Ok(lease)
    }

    fn restore(
        &mut self,
        operation: Result<(), ChildContextRecordError>,
    ) -> Result<(), ChildContextRecordError> {
        let restored = self
            .directory
            .sync_all()
            .map_err(|_error| ChildContextRecordError::CannotRecord)
            .and_then(|()| {
                nix::unistd::fchown(
                    self.directory,
                    Some(nix::unistd::Uid::from_raw(self.original.st_uid)),
                    Some(nix::unistd::Gid::from_raw(self.original.st_gid)),
                )
                .map_err(|_error| ChildContextRecordError::CannotRecord)
            })
            .and_then(|()| {
                nix::sys::stat::fchmod(
                    self.directory,
                    nix::sys::stat::Mode::from_bits_truncate(self.original.st_mode & 0o7777),
                )
                .map_err(|_error| ChildContextRecordError::CannotRecord)
            })
            .and_then(|()| {
                let current = nix::sys::stat::fstat(self.directory)
                    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
                let path = nix::sys::stat::fstatat(
                    self.parent,
                    self.name,
                    nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
                )
                .map_err(|_error| ChildContextRecordError::CannotRecord)?;
                if (
                    current.st_dev,
                    current.st_ino,
                    current.st_uid,
                    current.st_gid,
                ) != (
                    self.original.st_dev,
                    self.original.st_ino,
                    self.original.st_uid,
                    self.original.st_gid,
                ) || current.st_mode & 0o7777 != self.original.st_mode & 0o7777
                    || (path.st_dev, path.st_ino) != (self.original.st_dev, self.original.st_ino)
                {
                    return Err(ChildContextRecordError::CannotRecord);
                }
                Ok(())
            });
        if restored.is_ok() {
            self.restored = true;
        }
        restored.and(operation)
    }
}

impl Drop for ChildPrivilegeLease<'_> {
    fn drop(&mut self) {
        if !self.restored {
            match self.restore(Err(ChildContextRecordError::CannotRecord)) {
                Ok(()) | Err(_) => {}
            }
        }
    }
}

/// Opens a fresh plain child channel and binds an inode receipt to it.
pub fn child_handoff_receipt(path: &Path) -> Result<ChildHandoffReceipt, ChildContextRecordError> {
    let directory =
        open_plain_directory(path).map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let metadata = directory
        .metadata()
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    Ok(ChildHandoffReceipt {
        path: path.to_owned(),
        dev: metadata.dev(),
        ino: metadata.ino(),
        guard: read_child_receipt_guard(&directory)?,
    })
}

fn open_child_channel(
    receipt: &ChildHandoffReceipt,
) -> Result<(fs::File, String, fs::File), ChildContextRecordError> {
    let parent_path = receipt
        .path
        .parent()
        .ok_or(ChildContextRecordError::CannotRecord)?;
    let parent = open_plain_directory(parent_path)
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let name = receipt
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ChildContextRecordError::CannotRecord)?
        .to_owned();
    let child = nix::fcntl::openat(
        &parent,
        name.as_str(),
        nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_DIRECTORY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map(fs::File::from)
    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let metadata = child
        .metadata()
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    if (metadata.dev(), metadata.ino()) != (receipt.dev, receipt.ino) {
        return Err(ChildContextRecordError::CannotRecord);
    }
    verify_child_receipt_guard(&child, receipt)?;
    Ok((parent, name, child))
}

/// Reads a terminal child status only from the exact receipt-bound channel.
pub fn read_child_terminal_status(
    receipt: &ChildHandoffReceipt,
    expected_agent: &str,
    expected_session: &str,
) -> Result<ChildContextStatus, ChildContextRecordError> {
    validate_child_context_names("terminal", expected_agent, expected_session)?;
    let (parent, name, child) = open_child_channel(receipt)?;
    for (file, expected) in [("agent", expected_agent), ("session", expected_session)] {
        let before =
            nix::sys::stat::fstatat(&child, file, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
                .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        let value = support::plain::read_small_text_file_at(
            &child,
            file,
            4096,
            "invalid child terminal field",
        )
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        let after = nix::sys::stat::fstatat(&child, file, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
            .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        if value.trim() != expected
            || (before.st_dev, before.st_ino) != (after.st_dev, after.st_ino)
        {
            return Err(ChildContextRecordError::CannotRecord);
        }
    }
    let before =
        nix::sys::stat::fstatat(&child, "status", nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
            .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let value = support::plain::read_small_text_file_at(
        &child,
        "status",
        64,
        "invalid child terminal status",
    )
    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let after = nix::sys::stat::fstatat(&child, "status", nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let channel = nix::sys::stat::fstatat(
        &parent,
        name.as_str(),
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    if (before.st_dev, before.st_ino) != (after.st_dev, after.st_ino)
        || (channel.st_dev, channel.st_ino) != (receipt.dev, receipt.ino)
    {
        return Err(ChildContextRecordError::CannotRecord);
    }
    match ChildContextStatus::parse(value.trim()) {
        Some(status @ (ChildContextStatus::Done | ChildContextStatus::Error)) => Ok(status),
        _ => Err(ChildContextRecordError::InvalidStatus),
    }
}

/// Atomically finishes one receipt-bound child channel.
///
/// Result artifacts are published before the terminal status, so readers that
/// observe a terminal status always observe the matching result and refs.
pub fn finish_child_result(
    receipt: &ChildHandoffReceipt,
    child_agent: &str,
    child_session: &str,
    status: ChildContextStatus,
    result: &str,
    refs_jsonl: &str,
) -> Result<(), ChildContextRecordError> {
    finish_child_result_with_hook(
        receipt,
        child_agent,
        child_session,
        status,
        result,
        refs_jsonl,
        |_stage| Ok(()),
    )
}

pub(crate) fn finish_child_result_exclusive(
    receipt: &ChildHandoffReceipt,
    child_agent: &str,
    child_session: &str,
    status: ChildContextStatus,
    result: &str,
    refs_jsonl: &str,
) -> Result<(), ChildContextRecordError> {
    finish_child_result_with_mode_hook(
        receipt,
        child_agent,
        child_session,
        None,
        status,
        result,
        refs_jsonl,
        true,
        None,
        |_stage| Ok(()),
    )
}

/// Exclusive lock retained for one receipt-bound child context channel.
#[derive(Debug)]
pub struct ChildContextLease {
    child: nix::fcntl::Flock<fs::File>,
}

pub fn child_context_lease_status(
    lease: &ChildContextLease,
) -> Result<ChildContextStatus, ChildContextRecordError> {
    let value =
        support::plain::read_small_text_file_at(&lease.child, "status", 64, "invalid child status")
            .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    ChildContextStatus::parse(value.trim()).ok_or(ChildContextRecordError::InvalidStatus)
}

pub fn acquire_child_context_lease(
    receipt: &ChildHandoffReceipt,
) -> Result<ChildContextLease, ChildContextRecordError> {
    let (_, _, child) = open_child_channel(receipt)?;
    let child = nix::fcntl::Flock::lock(child, nix::fcntl::FlockArg::LockExclusive)
        .map_err(|(_file, _error)| ChildContextRecordError::CannotRecord)?;
    Ok(ChildContextLease { child })
}

struct ChildFieldReceipt {
    file: &'static str,
    dev: u64,
    ino: u64,
}

fn child_context_field_receipts(
    child: &fs::File,
    child_agent: &str,
    child_session: &str,
    expected_handoff: Option<&str>,
) -> Result<Vec<ChildFieldReceipt>, ChildContextRecordError> {
    let expected_handoff = expected_handoff.map(ensure_trailing_newline);
    let mut fields = vec![
        ("agent", child_agent, true),
        ("session", child_session, true),
    ];
    if let Some(handoff) = expected_handoff.as_deref() {
        fields.push(("handoff.md", handoff, false));
    }
    fields
        .into_iter()
        .map(|(file, expected, trimmed)| {
            let before =
                nix::sys::stat::fstatat(child, file, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
                    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
            let value = support::plain::read_small_text_file_at(
                child,
                file,
                65_537,
                "invalid child context field",
            )
            .map_err(|_error| ChildContextRecordError::CannotRecord)?;
            let after =
                nix::sys::stat::fstatat(child, file, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
                    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
            let matches = if trimmed {
                value.trim() == expected
            } else {
                value == expected
            };
            if !matches || !same_file(&before, &after) {
                return Err(ChildContextRecordError::InvalidStatus);
            }
            Ok(ChildFieldReceipt {
                file,
                dev: after.st_dev,
                ino: after.st_ino,
            })
        })
        .collect()
}

fn verify_child_context_lease(
    lease: &ChildContextLease,
    receipt: &ChildHandoffReceipt,
) -> Result<(), ChildContextRecordError> {
    let child = nix::sys::stat::fstat(&*lease.child)
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let parent_path = receipt
        .path
        .parent()
        .ok_or(ChildContextRecordError::CannotRecord)?;
    let parent = open_plain_directory(parent_path)
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let name = receipt
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ChildContextRecordError::CannotRecord)?;
    let current = nix::sys::stat::fstatat(&parent, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    if !is_plain_channel_directory(&child)
        || (child.st_dev, child.st_ino) != (receipt.dev, receipt.ino)
        || (current.st_dev, current.st_ino) != (receipt.dev, receipt.ino)
    {
        return Err(ChildContextRecordError::CannotRecord);
    }
    verify_child_receipt_guard(&lease.child, receipt)?;
    Ok(())
}

/// Validates immutable child identity and handoff fields under an exact channel lease.
pub fn validate_child_context_lease(
    lease: &ChildContextLease,
    receipt: &ChildHandoffReceipt,
    child_agent: &str,
    child_session: &str,
    expected_handoff: &str,
) -> Result<(), ChildContextRecordError> {
    validate_child_context_names("lease", child_agent, child_session)?;
    verify_child_context_lease(lease, receipt)?;
    child_context_field_receipts(
        &lease.child,
        child_agent,
        child_session,
        Some(expected_handoff),
    )?;
    verify_child_context_lease(lease, receipt)
}

#[expect(
    clippy::too_many_arguments,
    reason = "receipt-bound completion keeps lease and result fields explicit"
)]
pub fn finish_child_result_with_lease(
    lease: ChildContextLease,
    receipt: &ChildHandoffReceipt,
    child_agent: &str,
    child_session: &str,
    expected_handoff: Option<&str>,
    status: ChildContextStatus,
    result: &str,
    refs_jsonl: &str,
) -> Result<(), ChildContextRecordError> {
    let exclusive = nix::unistd::Uid::effective().is_root();
    finish_child_result_with_mode_hook(
        receipt,
        child_agent,
        child_session,
        expected_handoff,
        status,
        result,
        refs_jsonl,
        exclusive,
        Some(lease),
        |_stage| Ok(()),
    )
}

#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "fault injection exercises the same ordered receipt-bound transaction"
)]
pub(crate) fn finish_child_result_with_hook(
    receipt: &ChildHandoffReceipt,
    child_agent: &str,
    child_session: &str,
    status: ChildContextStatus,
    result: &str,
    refs_jsonl: &str,
    hook: impl FnMut(ChildFinishStage) -> Result<(), ChildContextRecordError>,
) -> Result<(), ChildContextRecordError> {
    finish_child_result_with_mode_hook(
        receipt,
        child_agent,
        child_session,
        None,
        status,
        result,
        refs_jsonl,
        false,
        None,
        hook,
    )
}

#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "fault injection exercises the same ordered receipt-bound transaction"
)]
fn finish_child_result_with_mode_hook(
    receipt: &ChildHandoffReceipt,
    child_agent: &str,
    child_session: &str,
    expected_handoff: Option<&str>,
    status: ChildContextStatus,
    result: &str,
    refs_jsonl: &str,
    exclusive: bool,
    locked: Option<ChildContextLease>,
    mut hook: impl FnMut(ChildFinishStage) -> Result<(), ChildContextRecordError>,
) -> Result<(), ChildContextRecordError> {
    validate_child_context_names("finish", child_agent, child_session)?;
    if !matches!(
        status,
        ChildContextStatus::Done | ChildContextStatus::Error | ChildContextStatus::Cancelled
    ) {
        return Err(ChildContextRecordError::InvalidStatus);
    }
    if result.contains('\0') || refs_jsonl.contains('\0') {
        return Err(ChildContextRecordError::InvalidText);
    }
    if !inspect_context_jsonl(ContextJsonlKind::Refs, refs_jsonl).is_ok() {
        return Err(ChildContextRecordError::InvalidRefs);
    }
    let (parent, name, opened) = open_child_channel(receipt)?;
    let child_fd = match locked {
        Some(lease) => lease.child,
        None => nix::fcntl::Flock::lock(opened, nix::fcntl::FlockArg::LockExclusive)
            .map_err(|(_file, _error)| ChildContextRecordError::CannotRecord)?,
    };
    let child_dir = &*child_fd;
    let mut lease = exclusive
        .then(|| ChildPrivilegeLease::acquire(child_dir, &parent, name.as_str(), receipt))
        .transpose()?;
    let operation = (|| {
        let field_receipts =
            child_context_field_receipts(child_dir, child_agent, child_session, expected_handoff)?;
        let current = support::plain::read_small_text_file_at(
            &child_fd,
            "status",
            64,
            "invalid child status",
        )
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        let current = ChildContextStatus::parse(current.trim())
            .ok_or(ChildContextRecordError::InvalidStatus)?;
        if current != ChildContextStatus::Active
            && !(status == ChildContextStatus::Cancelled && current == ChildContextStatus::Pending)
        {
            return if current == status {
                let old_result = support::plain::read_small_text_file_at(
                    &child_fd,
                    "result.md",
                    65_537,
                    "invalid child result",
                )
                .map_err(|_error| ChildContextRecordError::CannotRecord)?;
                let old_refs = support::plain::read_small_text_file_at(
                    &child_fd,
                    "refs.jsonl",
                    65_537,
                    "invalid child refs",
                )
                .map_err(|_error| ChildContextRecordError::CannotRecord)?;
                if old_result == ensure_trailing_newline(result)
                    && old_refs == ensure_trailing_newline(refs_jsonl)
                {
                    Ok(())
                } else {
                    Err(ChildContextRecordError::InvalidStatus)
                }
            } else {
                Err(ChildContextRecordError::InvalidStatus)
            };
        }
        let status_receipt = nix::sys::stat::fstatat(
            child_dir,
            "status",
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        let mut artifact_receipts = Vec::new();
        for (file, value) in [
            ("result.md", ensure_trailing_newline(result)),
            ("refs.jsonl", ensure_trailing_newline(refs_jsonl)),
            ("status", format!("{}\n", status.as_str())),
        ] {
            if file == "status" {
                hook(ChildFinishStage::BeforeStatus)?;
                for receipt in &field_receipts {
                    let current = nix::sys::stat::fstatat(
                        child_dir,
                        receipt.file,
                        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
                    )
                    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
                    if (current.st_dev, current.st_ino) != (receipt.dev, receipt.ino) {
                        return Err(ChildContextRecordError::CannotRecord);
                    }
                }
                let current = nix::sys::stat::fstatat(
                    child_dir,
                    "status",
                    nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
                )
                .map_err(|_error| ChildContextRecordError::CannotRecord)?;
                if (current.st_dev, current.st_ino)
                    != (status_receipt.st_dev, status_receipt.st_ino)
                {
                    return Err(ChildContextRecordError::InvalidStatus);
                }
                let channel = nix::sys::stat::fstatat(
                    &parent,
                    name.as_str(),
                    nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
                )
                .map_err(|_error| ChildContextRecordError::CannotRecord)?;
                if (channel.st_dev, channel.st_ino) != (receipt.dev, receipt.ino) {
                    return Err(ChildContextRecordError::CannotRecord);
                }
                for &(artifact, dev, ino) in &artifact_receipts {
                    let current = nix::sys::stat::fstatat(
                        child_dir,
                        artifact,
                        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
                    )
                    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
                    if (current.st_dev, current.st_ino) != (dev, ino) {
                        return Err(ChildContextRecordError::CannotRecord);
                    }
                }
            }
            let temporary = format!(
                ".{file}.finish-{}-{}",
                std::process::id(),
                CHILD_STAGE_ID.fetch_add(1, Ordering::Relaxed)
            );
            replace_child_file(child_dir, file, &temporary, &value, |point| {
                match (file, point) {
                    ("result.md", ReplacePoint::Prepared) => {
                        hook(ChildFinishStage::BeforeResultPublish)
                    }
                    ("result.md", ReplacePoint::Rechecked) => {
                        hook(ChildFinishStage::AfterResultRecheck)
                    }
                    ("result.md", ReplacePoint::Exchanged) => {
                        hook(ChildFinishStage::AfterResultExchange)
                    }
                    ("result.md", ReplacePoint::Quarantined) => {
                        hook(ChildFinishStage::BeforeResultCleanup)
                    }
                    ("refs.jsonl", ReplacePoint::Prepared) => {
                        hook(ChildFinishStage::BeforeRefsPublish)
                    }
                    ("refs.jsonl", ReplacePoint::Rechecked) => {
                        hook(ChildFinishStage::AfterRefsRecheck)
                    }
                    ("refs.jsonl", ReplacePoint::Exchanged) => {
                        hook(ChildFinishStage::AfterRefsExchange)
                    }
                    ("refs.jsonl", ReplacePoint::Quarantined) => {
                        hook(ChildFinishStage::BeforeRefsCleanup)
                    }
                    ("status", ReplacePoint::Rechecked) => {
                        hook(ChildFinishStage::AfterStatusRecheck)
                    }
                    ("status", ReplacePoint::Exchanged) => {
                        hook(ChildFinishStage::AfterStatusExchange)
                    }
                    ("status", ReplacePoint::Quarantined) => {
                        hook(ChildFinishStage::BeforeStatusCleanup)
                    }
                    _ => Ok(()),
                }
            })?;
            if matches!(file, "result.md" | "refs.jsonl") {
                let published = nix::sys::stat::fstatat(
                    child_dir,
                    file,
                    nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
                )
                .map_err(|_error| ChildContextRecordError::CannotRecord)?;
                artifact_receipts.push((file, published.st_dev, published.st_ino));
                hook(if file == "result.md" {
                    ChildFinishStage::AfterResultPublish
                } else {
                    ChildFinishStage::AfterRefsPublish
                })?;
            }
        }
        Ok(())
    })();
    lease
        .as_mut()
        .map_or(operation, |lease| lease.restore(operation))
}

/// Exclusively publishes a complete child handoff and returns its inode receipt.
pub fn publish_child_handoff(
    parent_session_dir: &Path,
    child_name: &str,
    child_agent: &str,
    child_session: &str,
    handoff: &str,
) -> Result<ChildHandoffReceipt, ChildContextRecordError> {
    publish_child_handoff_with_hook(
        parent_session_dir,
        child_name,
        child_agent,
        child_session,
        handoff,
        |_stage| Ok(()),
    )
}

pub(crate) fn publish_child_handoff_with_hook(
    parent_session_dir: &Path,
    child_name: &str,
    child_agent: &str,
    child_session: &str,
    handoff: &str,
    mut hook: impl FnMut(ChildHandoffStage) -> Result<(), ChildContextRecordError>,
) -> Result<ChildHandoffReceipt, ChildContextRecordError> {
    validate_child_context_names(child_name, child_agent, child_session)?;
    if handoff.contains('\0') {
        return Err(ChildContextRecordError::InvalidText);
    }
    require_parent_session_context(parent_session_dir)?;
    let child_parent = parent_session_dir.join("context/child");
    let parent = open_plain_directory(&child_parent)
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let stage_name = format!(
        ".{child_name}.stage-{}-{}",
        std::process::id(),
        CHILD_STAGE_ID.fetch_add(1, Ordering::Relaxed)
    );
    let stage = child_parent.join(&stage_name);
    hook(ChildHandoffStage::Staging)?;
    let guard = support::receipt::random_hex::<CHILD_RECEIPT_BYTES>()
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let stage_fd = support::plain::create_plain_dir_exclusive(&stage, 0o700)
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    support::plain::write_text_file_at(
        &stage_fd,
        CHILD_RECEIPT_FILE,
        &ensure_trailing_newline(&guard),
        0o600,
    )
    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let stage_metadata = stage_fd
        .metadata()
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let stage_receipt = ChildHandoffReceipt {
        path: stage,
        dev: stage_metadata.dev(),
        ino: stage_metadata.ino(),
        guard: Some(guard.clone()),
    };
    let result = (|| {
        hook(ChildHandoffStage::Artifact)?;
        support::plain::create_plain_dir_at(&stage_fd, "artifact", 0o700)
            .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        for (stage_kind, file, value) in [
            (ChildHandoffStage::Agent, "agent", child_agent),
            (ChildHandoffStage::Session, "session", child_session),
            (
                ChildHandoffStage::Status,
                "status",
                ChildContextStatus::Pending.as_str(),
            ),
            (ChildHandoffStage::Handoff, "handoff.md", handoff),
            (ChildHandoffStage::Result, "result.md", ""),
            (ChildHandoffStage::Refs, "refs.jsonl", ""),
        ] {
            hook(stage_kind)?;
            support::plain::write_text_file_at(
                &stage_fd,
                file,
                &ensure_trailing_newline(value),
                0o600,
            )
            .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        }
        hook(ChildHandoffStage::Publish)?;
        if open_child_channel(&stage_receipt).is_err() {
            return Err(ChildContextRecordError::CannotRecord);
        }
        nix::fcntl::renameat2(
            &parent,
            stage_name.as_str(),
            &parent,
            child_name,
            nix::fcntl::RenameFlags::RENAME_NOREPLACE,
        )
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
        let path = child_parent.join(child_name);
        let receipt = child_handoff_receipt(&path)?;
        if receipt.guard.as_deref() != Some(guard.as_str()) {
            return Err(ChildContextRecordError::CannotRecord);
        }
        Ok(receipt)
    })();
    if result.is_err() {
        let _rollback = rollback_child_handoff(&stage_receipt);
    }
    result
}

/// Rolls back a published handoff only while its inode still matches the receipt.
pub fn rollback_child_handoff(
    receipt: &ChildHandoffReceipt,
) -> Result<(), ChildContextRecordError> {
    if receipt.guard.is_none() {
        return Err(ChildContextRecordError::CannotRecord);
    }
    let parent_path = receipt
        .path
        .parent()
        .ok_or(ChildContextRecordError::CannotRecord)?;
    let parent = open_plain_directory(parent_path)
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let name = receipt
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ChildContextRecordError::CannotRecord)?;
    let quarantine = format!(
        ".{name}.rollback-{}-{}",
        std::process::id(),
        CHILD_STAGE_ID.fetch_add(1, Ordering::Relaxed)
    );
    nix::fcntl::renameat2(
        &parent,
        name,
        &parent,
        quarantine.as_str(),
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let quarantine_path = parent_path.join(&quarantine);
    let matches = open_plain_directory(&quarantine_path).is_ok_and(|child| {
        child.metadata().is_ok_and(|metadata| {
            metadata.is_dir()
                && (metadata.dev(), metadata.ino()) == (receipt.dev, receipt.ino)
                && verify_child_receipt_guard(&child, receipt).is_ok()
        })
    });
    if !matches {
        let _ignored = nix::fcntl::renameat2(
            &parent,
            quarantine.as_str(),
            &parent,
            name,
            nix::fcntl::RenameFlags::RENAME_NOREPLACE,
        );
        return Err(ChildContextRecordError::CannotRecord);
    }
    drop(fs::remove_dir_all(quarantine_path));
    Ok(())
}

/// Claims a receipt-bound pending handoff after its exact child runtime is ready.
pub fn claim_child_handoff_active(
    receipt: &ChildHandoffReceipt,
    child_agent: &str,
    child_session: &str,
    expected_handoff: Option<&str>,
) -> Result<(), ChildContextRecordError> {
    let lease = acquire_child_context_lease(receipt)?;
    claim_child_handoff_active_with_lease_hook(
        &lease,
        receipt,
        child_agent,
        child_session,
        expected_handoff,
        |_stage| Ok(()),
    )
}

#[cfg(test)]
pub(crate) fn claim_child_handoff_active_with_hook(
    receipt: &ChildHandoffReceipt,
    child_agent: &str,
    child_session: &str,
    expected_handoff: Option<&str>,
    hook: impl FnMut(ChildClaimStage) -> Result<(), ChildContextRecordError>,
) -> Result<(), ChildContextRecordError> {
    let lease = acquire_child_context_lease(receipt)?;
    claim_child_handoff_active_with_lease_hook(
        &lease,
        receipt,
        child_agent,
        child_session,
        expected_handoff,
        hook,
    )
}

/// Claims one pending child while retaining the caller's exact channel lease.
pub fn claim_child_handoff_active_with_lease(
    lease: &ChildContextLease,
    receipt: &ChildHandoffReceipt,
    child_agent: &str,
    child_session: &str,
    expected_handoff: Option<&str>,
) -> Result<(), ChildContextRecordError> {
    claim_child_handoff_active_with_lease_hook(
        lease,
        receipt,
        child_agent,
        child_session,
        expected_handoff,
        |_stage| Ok(()),
    )
}

fn claim_child_handoff_active_with_lease_hook(
    lease: &ChildContextLease,
    receipt: &ChildHandoffReceipt,
    child_agent: &str,
    child_session: &str,
    expected_handoff: Option<&str>,
    mut hook: impl FnMut(ChildClaimStage) -> Result<(), ChildContextRecordError>,
) -> Result<(), ChildContextRecordError> {
    validate_child_context_names("claim", child_agent, child_session)?;
    verify_child_context_lease(lease, receipt)?;
    let child_fd = &lease.child;
    let fields =
        child_context_field_receipts(child_fd, child_agent, child_session, expected_handoff)?;
    let status_before = nix::sys::stat::fstatat(
        &**child_fd,
        "status",
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let status = support::plain::read_small_text_file_at(
        child_fd,
        "status",
        64,
        "invalid child claim status",
    )
    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let status_after = nix::sys::stat::fstatat(
        &**child_fd,
        "status",
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    if !same_file(&status_before, &status_after) {
        return Err(ChildContextRecordError::CannotRecord);
    }
    match ChildContextStatus::parse(status.trim()) {
        Some(ChildContextStatus::Pending) => {}
        Some(
            ChildContextStatus::Active
            | ChildContextStatus::Done
            | ChildContextStatus::Error
            | ChildContextStatus::Cancelled,
        )
        | None => return Err(ChildContextRecordError::InvalidStatus),
    }
    let temporary = format!(
        ".status.claim-{}-{}",
        std::process::id(),
        CHILD_STAGE_ID.fetch_add(1, Ordering::Relaxed)
    );
    replace_child_file(
        child_fd,
        "status",
        &temporary,
        "active\n",
        |point| match point {
            ReplacePoint::Prepared => hook(ChildClaimStage::Staging),
            ReplacePoint::Rechecked => {
                hook(ChildClaimStage::Publish)?;
                for receipt in &fields {
                    let current = nix::sys::stat::fstatat(
                        &**child_fd,
                        receipt.file,
                        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
                    )
                    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
                    if (current.st_dev, current.st_ino) != (receipt.dev, receipt.ino) {
                        return Err(ChildContextRecordError::CannotRecord);
                    }
                }
                verify_child_context_lease(lease, receipt)
            }
            ReplacePoint::Exchanged | ReplacePoint::Quarantined => Ok(()),
        },
    )?;
    verify_child_context_lease(lease, receipt)
}

/// Creates or replaces the parent-owned child handoff channel.
///
/// This writes only the documented `context/child/<child>/` files under the
/// parent session. It does not copy parent `messages.jsonl`, preserving the
/// child-context isolation rule.
pub fn record_child_handoff_to_parent_context(
    parent_session_dir: &Path,
    child_name: &str,
    child_agent: &str,
    child_session: &str,
    handoff: &str,
) -> Result<(), ChildContextRecordError> {
    validate_child_context_names(child_name, child_agent, child_session)?;
    if handoff.contains('\0') {
        return Err(ChildContextRecordError::InvalidText);
    }
    require_parent_session_context(parent_session_dir)?;

    let child_dir = parent_session_dir.join("context/child").join(child_name);
    create_private_context_dir(&child_dir)
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    create_private_context_dir(&child_dir.join("artifact"))
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    for (file, value) in [
        ("agent", child_agent),
        ("session", child_session),
        ("status", ChildContextStatus::Pending.as_str()),
    ] {
        write_child_context_file(&child_dir, file, &format!("{value}\n"))?;
    }
    write_child_context_file(&child_dir, "handoff.md", &ensure_trailing_newline(handoff))?;
    write_text_file_if_absent(&child_dir.join("result.md"), "")
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    write_text_file_if_absent(&child_dir.join("refs.jsonl"), "")
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;

    Ok(())
}

/// Records a child result back into the parent session's child channel.
///
/// The result and refs are inspectable from the parent context pack through
/// `context/child/<child>/result.md` and `refs.jsonl`. This helper keeps the
/// child's full durable history in the child session, not in the parent pack.
pub fn record_child_result_to_parent_context(
    parent_session_dir: &Path,
    child_name: &str,
    status: ChildContextStatus,
    result: &str,
    refs_jsonl: &str,
) -> Result<(), ChildContextRecordError> {
    if !is_object_name(child_name) {
        return Err(ChildContextRecordError::InvalidChildName);
    }
    let child_dir = parent_session_dir.join("context/child").join(child_name);
    require_child_context_files(&child_dir)?;
    let child_agent = fs::read_to_string(child_dir.join("agent"))
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let child_session = fs::read_to_string(child_dir.join("session"))
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    let child_agent = child_agent.trim();
    let child_session = child_session.trim();
    validate_child_context_names(child_name, child_agent, child_session)?;
    let receipt = child_handoff_receipt(&child_dir)?;
    finish_child_result(
        &receipt,
        child_agent,
        child_session,
        status,
        result,
        refs_jsonl,
    )
}

/// Validates and records a parent-owned hybrid DAG/ReAct schedule.
///
/// The schedule is ordinary parent session context at `context/plan.json`.
/// Recording it does not create agents, enqueue jobs, start a watcher, or grant
/// authority; every declared `requires` entry must already be allowed by the
/// parent effective policy.
pub fn record_agent_schedule_to_parent_context(
    parent_session_dir: &Path,
    schedule_json: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
) -> Result<(), AgentScheduleRecordError> {
    if schedule_json.contains('\0') {
        return Err(AgentScheduleRecordError::InvalidText);
    }
    let report = inspect_agent_schedule_json(schedule_json, parent_subject, parent_policy);
    if !report.is_ok() {
        return Err(AgentScheduleRecordError::InvalidSchedule(report));
    }
    require_agent_schedule_parent_context(parent_session_dir)?;
    atomic_replace_text(
        &parent_session_dir.join("context").join("plan.json"),
        &ensure_trailing_newline(schedule_json),
    )
    .map_err(|_error| AgentScheduleRecordError::CannotRecord)
}

/// Records ready delegated schedule nodes into parent child handoff channels.
///
/// This materializes only parent-owned handoff files for ready nodes that
/// declare `child` and `handoff` in `context/plan.json`. It does not create or
/// start child agents and does not mark schedule nodes complete.
pub fn record_ready_agent_schedule_child_handoffs_to_parent_context(
    parent_session_dir: &Path,
    schedule_json: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
    completed_nodes: &[&str],
) -> Result<Vec<AgentScheduleChildHandoff>, AgentScheduleRecordError> {
    let default_child_session = parent_session_name(parent_session_dir)?;
    let handoffs = ready_agent_schedule_child_handoffs(
        schedule_json,
        parent_subject,
        parent_policy,
        completed_nodes,
        &default_child_session,
    )
    .map_err(AgentScheduleRecordError::InvalidSchedule)?;
    require_agent_schedule_parent_context(parent_session_dir)?;

    let mut recorded = Vec::new();
    for handoff in handoffs {
        if schedule_child_handoff_materialized(parent_session_dir, &handoff)? {
            continue;
        }
        record_child_handoff_to_parent_context(
            parent_session_dir,
            handoff.child(),
            handoff.agent(),
            handoff.session(),
            handoff.handoff(),
        )
        .map_err(agent_schedule_child_record_error)?;
        recorded.push(handoff);
    }

    Ok(recorded)
}

/// Derives completed hybrid schedule nodes from durable parent-visible state.
///
/// Local parent-owned node completions are supplied explicitly. Delegated nodes
/// are complete when their `context/child/<child>/status` file is a plain file
/// containing `done`.
pub fn completed_agent_schedule_nodes_from_parent_context(
    parent_session_dir: &Path,
    schedule_json: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
    local_completed_nodes: &[&str],
) -> Result<Vec<String>, AgentScheduleRecordError> {
    let nodes = agent_schedule_nodes(schedule_json, parent_subject, parent_policy)
        .map_err(AgentScheduleRecordError::InvalidSchedule)?;
    require_agent_schedule_parent_context(parent_session_dir)?;

    let known = nodes
        .iter()
        .map(AgentScheduleNode::id)
        .collect::<HashSet<_>>();
    let mut completed = Vec::new();
    let mut seen = HashSet::new();
    let mut issues = Vec::new();
    for node in local_completed_nodes {
        if !is_object_name(node) || !known.contains(*node) {
            issues.push(AgentScheduleIssue::UnknownCompletedNode {
                node: (*node).to_owned(),
            });
        } else if nodes
            .iter()
            .any(|candidate| candidate.id() == *node && candidate.child().is_some())
        {
            issues.push(AgentScheduleIssue::DelegatedCompletionRequiresChildResult {
                node: (*node).to_owned(),
            });
        } else if seen.insert((*node).to_owned()) {
            completed.push((*node).to_owned());
        }
    }
    if !issues.is_empty() {
        return Err(AgentScheduleRecordError::InvalidSchedule(
            AgentScheduleReport::new(issues),
        ));
    }

    let default_child_session = parent_session_name(parent_session_dir)?;
    for node in nodes {
        let Some(child) = node.child() else {
            continue;
        };
        let child_dir = parent_session_dir.join("context").join("child").join(child);
        let Some(handoff) = node.handoff() else {
            return Err(AgentScheduleRecordError::CannotRecord);
        };
        let child_session = node.child_session().unwrap_or(&default_child_session);
        if !schedule_child_context_matches(
            parent_session_dir,
            child,
            node.agent(),
            child_session,
            handoff,
        )? {
            continue;
        }
        match read_child_schedule_status(&child_dir)? {
            Some(ChildContextStatus::Done) if seen.insert(node.id().to_owned()) => {
                completed.push(node.id().to_owned());
            }
            Some(_) | None => {}
        }
    }

    Ok(completed)
}

pub(crate) fn parent_session_name(
    parent_session_dir: &Path,
) -> Result<String, AgentScheduleRecordError> {
    let Some(name) = parent_session_dir
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return Err(AgentScheduleRecordError::MissingParentSession);
    };
    if !is_object_name(name) {
        return Err(AgentScheduleRecordError::MissingParentSession);
    }
    Ok(name.to_owned())
}

/// Advances a parent-owned hybrid schedule from durable parent context.
///
/// This reads delegated child statuses, combines them with explicit local
/// completions, and materializes ready delegated handoffs. It is a single
/// parent-session state transition helper, not a scheduler loop.
pub fn advance_agent_schedule_from_parent_context(
    parent_session_dir: &Path,
    schedule_json: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
    local_completed_nodes: &[&str],
) -> Result<AgentScheduleAdvance, AgentScheduleRecordError> {
    let completed_nodes = completed_agent_schedule_nodes_from_parent_context(
        parent_session_dir,
        schedule_json,
        parent_subject,
        parent_policy,
        local_completed_nodes,
    )?;
    let completed_refs = completed_nodes
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let handoffs = record_ready_agent_schedule_child_handoffs_to_parent_context(
        parent_session_dir,
        schedule_json,
        parent_subject,
        parent_policy,
        &completed_refs,
    )?;

    Ok(AgentScheduleAdvance::new(completed_nodes, handoffs))
}

/// Derives the stable `session/index/by-cwd/<key>` file name for a chroot cwd.
#[must_use]
pub fn session_index_key_for_cwd(cwd: &str) -> Option<String> {
    if !is_stable_chroot_absolute_path(cwd) {
        return None;
    }
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in cwd.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Some(format!("cwd-{hash:016x}"))
}

#[expect(
    clippy::too_many_arguments,
    reason = "durable send recording keeps validated request fields explicit"
)]
pub(crate) fn record_socket_send_to_session(
    session_dir: &Path,
    client_id: &str,
    run_id: &str,
    session: &str,
    scope: SocketSessionScope,
    cwd: Option<&str>,
    input: &str,
    preparation: Option<&OwnedSessionPreparation>,
    locked_history: Option<&columnar::HistoryGuard<'_>>,
) -> Result<SocketSessionRecord, SocketSessionRecordError> {
    validate_socket_send(session_dir, client_id, session, scope, input)?;
    validate_socket_object_field("run", run_id)
        .map_err(|_error| SocketSessionRecordError::InvalidField("run"))?;

    let message = serde_json::json!({
        "role": "user",
        "run": run_id,
        "content": input
    })
    .to_string();
    let event = serde_json::json!({
        "type": "start",
        "id": run_id,
        "run": run_id,
        "client_id": client_id,
        "scope": scope.as_str(),
        "cwd": cwd
    })
    .to_string();

    let owned_history;
    let indexed = locked_history.is_some();
    let history = if let Some(history) = locked_history {
        history
    } else {
        owned_history = columnar::HistoryGuard::exclusive(session_dir)
            .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
        &owned_history
    };
    if indexed {
        let send = match history
            .lookup_send(client_id, input, scope.as_str(), cwd)
            .map_err(|_error| SocketSessionRecordError::CannotRecord)?
        {
            columnar::SendClaim::Vacant => history
                .prepare_send(
                    client_id,
                    run_id,
                    input,
                    scope.as_str(),
                    cwd,
                    &message,
                    &event,
                )
                .map_err(|_error| SocketSessionRecordError::CannotRecord)?,
            columnar::SendClaim::Pending(send) if send.run_id() == run_id => send,
            columnar::SendClaim::Pending(_)
            | columnar::SendClaim::Replay(_)
            | columnar::SendClaim::Conflict
            | columnar::SendClaim::Corrupt => {
                return Err(SocketSessionRecordError::CannotRecord);
            }
        };
        history
            .append_prepared_send(&send, &message, &event)
            .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    } else {
        history
            .refresh_claims()
            .and_then(|()| history.append(columnar::Stream::Messages, &[&message]))
            .and_then(|()| history.append(columnar::Stream::Events, &[&event]))
            .and_then(|()| history.refresh_claims())
            .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    }
    set_active_session_run_locked(history, session_dir, run_id, preparation)?;
    if let Some(cwd) = cwd {
        write_session_file(session_dir, "cwd", &format!("{cwd}\n"))?;
    }

    Ok(SocketSessionRecord::new(vec![message], vec![event]))
}

pub(crate) fn validate_socket_send(
    session_dir: &Path,
    client_id: &str,
    session: &str,
    scope: SocketSessionScope,
    input: &str,
) -> Result<(), SocketSessionRecordError> {
    if scope == SocketSessionScope::Temp {
        return Err(SocketSessionRecordError::TempSessionNotDurable);
    }
    validate_socket_object_field("id", client_id)
        .map_err(|_error| SocketSessionRecordError::InvalidField("id"))?;
    if input.contains('\0') {
        return Err(SocketSessionRecordError::InvalidField("input"));
    }
    require_socket_session_name(session_dir, session)?;
    require_socket_session_files(session_dir)
}

pub(crate) fn record_socket_cancel_to_session(
    session_dir: &Path,
    run_id: &str,
) -> Result<SocketSessionRecord, SocketSessionRecordError> {
    validate_socket_object_field("id", run_id)
        .map_err(|_error| SocketSessionRecordError::InvalidField("id"))?;
    require_socket_session_files(session_dir)?;

    let history = columnar::HistoryGuard::exclusive(session_dir)
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    let Some(canonical_run) =
        resolve_active_session_cancel_run_locked(&history, session_dir, run_id)?
    else {
        return Err(SocketSessionRecordError::CannotRecord);
    };
    let event = done_event_json(&canonical_run, "cancelled");
    history
        .append(columnar::Stream::Events, &[&event])
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    set_session_state(session_dir, "cancelled")?;

    Ok(SocketSessionRecord::new(Vec::new(), vec![event]))
}

pub(crate) fn text_content_parts(content: &str) -> Value {
    serde_json::json!([{ "type": "text", "text": content }])
}

pub(crate) fn done_event_json(run_id: &str, status: &str) -> String {
    serde_json::json!({ "type": "done", "run": run_id, "status": status }).to_string()
}

pub(crate) fn validate_child_context_names(
    child_name: &str,
    child_agent: &str,
    child_session: &str,
) -> Result<(), ChildContextRecordError> {
    for (value, error) in [
        (child_name, ChildContextRecordError::InvalidChildName),
        (child_agent, ChildContextRecordError::InvalidAgentName),
        (child_session, ChildContextRecordError::InvalidSessionName),
    ] {
        if !is_object_name(value) {
            return Err(error);
        }
    }
    Ok(())
}
