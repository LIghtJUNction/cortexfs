use crate::*;

use crate::support::plain::{open_plain_directory, path_metadata_no_follow, plain_file_name};
#[cfg(test)]
use std::os::unix::fs::FileExt;

#[cfg(test)]
pub(crate) fn append_jsonl_line(path: &Path, line: &str) -> std::io::Result<()> {
    if line.bytes().any(|byte| matches!(byte, b'\n' | b'\r')) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "jsonl line contains a line break",
        ));
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_dir = open_plain_directory(parent)?;
    let file_name = plain_file_name(path)?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_APPEND
            | nix::fcntl::OFlag::O_RDWR
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(nix_errno_to_io)?;
    let mut file = fs::File::from(file_fd);
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::other("jsonl target is not a regular file"));
    }
    if metadata.len() != 0 {
        let mut last = [0_u8; 1];
        if file.read_at(&mut last, metadata.len() - 1)? != 1 || last[0] != b'\n' {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "jsonl target has an incomplete final line",
            ));
        }
    }
    let mut frame = line.as_bytes().to_vec();
    frame.push(b'\n');
    file.write_all(&frame)?;
    file.flush()?;
    file.sync_all()
}

pub(crate) fn atomic_replace_text(path: &Path, content: &str) -> std::io::Result<()> {
    atomic_replace_text_with_mode(path, content, 0o600)
}

pub fn atomic_replace_text_with_mode(path: &Path, content: &str, mode: u32) -> std::io::Result<()> {
    atomic_replace_text_inner(path, content, AtomicReplaceMetadata::mode(mode), None)
}

pub fn atomic_create_text_with_mode(path: &Path, content: &str, mode: u32) -> std::io::Result<()> {
    atomic_replace_text_inner(path, content, AtomicReplaceMetadata::create(mode), None)
}

pub fn atomic_replace_text_preserving_metadata(path: &Path, content: &str) -> std::io::Result<()> {
    atomic_replace_text_preserving_metadata_inner(path, content, None, None)
}

pub fn atomic_replace_text_preserving_metadata_if_matches(
    path: &Path,
    content: &str,
    expected: (u64, u64),
) -> std::io::Result<()> {
    atomic_replace_text_preserving_metadata_inner(path, content, Some(expected), None)
}

fn atomic_replace_text_preserving_metadata_inner(
    path: &Path,
    content: &str,
    expected: Option<(u64, u64)>,
    before_commit: Option<&mut dyn FnMut() -> std::io::Result<()>>,
) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_dir = open_plain_directory(parent)?;
    let file_name = plain_file_name(path)?;
    let existing_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_WRONLY
            | nix::fcntl::OFlag::O_NONBLOCK
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(nix_errno_to_io)?;
    let existing = fs::File::from(existing_fd);
    let metadata = existing.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::other(
            "atomic replace target is not a regular file",
        ));
    }
    let identity = (metadata.dev(), metadata.ino());
    if expected.is_some_and(|expected| expected != identity) {
        return Err(std::io::Error::other("atomic replace target changed"));
    }
    let replacement = AtomicReplaceMetadata {
        mode: metadata.permissions().mode() & 0o7777,
        owner: Some((metadata.uid(), metadata.gid())),
        identity: Some(expected.unwrap_or(identity)),
        commit: AtomicCommit::Replace,
    };
    drop(existing);
    atomic_replace_text_in_parent(&parent_dir, file_name, content, replacement, before_commit)
}

#[cfg(test)]
pub(crate) fn atomic_replace_text_preserving_metadata_with_hook(
    path: &Path,
    content: &str,
    before_commit: &mut dyn FnMut() -> std::io::Result<()>,
) -> std::io::Result<()> {
    atomic_replace_text_preserving_metadata_inner(path, content, None, Some(before_commit))
}

#[derive(Clone, Copy)]
struct AtomicReplaceMetadata {
    mode: u32,
    owner: Option<(u32, u32)>,
    identity: Option<(u64, u64)>,
    commit: AtomicCommit,
}

#[derive(Clone, Copy)]
enum AtomicCommit {
    Replace,
    NoReplace,
}

impl AtomicReplaceMetadata {
    const fn mode(mode: u32) -> Self {
        Self {
            mode,
            owner: None,
            identity: None,
            commit: AtomicCommit::Replace,
        }
    }

    const fn create(mode: u32) -> Self {
        Self {
            mode,
            owner: None,
            identity: None,
            commit: AtomicCommit::NoReplace,
        }
    }
}

fn atomic_replace_text_inner(
    path: &Path,
    content: &str,
    metadata: AtomicReplaceMetadata,
    before_commit: Option<&mut dyn FnMut() -> std::io::Result<()>>,
) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_dir = open_plain_directory(parent)?;
    let file_name = plain_file_name(path)?;
    atomic_replace_text_in_parent(&parent_dir, file_name, content, metadata, before_commit)
}

fn atomic_replace_text_in_parent(
    parent_dir: &fs::File,
    file_name: &str,
    content: &str,
    metadata: AtomicReplaceMetadata,
    mut before_commit: Option<&mut dyn FnMut() -> std::io::Result<()>>,
) -> std::io::Result<()> {
    for attempt in 0..16 {
        let temp_name = generated_sibling_name(file_name, "tmp", attempt);
        let file_fd = match nix::fcntl::openat(
            parent_dir,
            temp_name.as_str(),
            nix::fcntl::OFlag::O_CREAT
                | nix::fcntl::OFlag::O_EXCL
                | nix::fcntl::OFlag::O_WRONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::from_bits_truncate(metadata.mode),
        ) {
            Ok(file_fd) => file_fd,
            Err(nix::errno::Errno::EEXIST) => continue,
            Err(error) => return Err(nix_errno_to_io(error)),
        };
        let mut file = fs::File::from(file_fd);
        if let Some((uid, gid)) = metadata.owner {
            let created = match file.metadata() {
                Ok(created) => created,
                Err(error) => {
                    remove_atomic_temp(parent_dir, &temp_name);
                    return Err(error);
                }
            };
            if (created.uid(), created.gid()) != (uid, gid)
                && let Err(error) = nix::unistd::fchown(
                    &file,
                    Some(nix::unistd::Uid::from_raw(uid)),
                    Some(nix::unistd::Gid::from_raw(gid)),
                )
            {
                remove_atomic_temp(parent_dir, &temp_name);
                return Err(nix_errno_to_io(error));
            }
        }
        if let Err(error) = file.set_permissions(fs::Permissions::from_mode(metadata.mode & 0o7777))
        {
            remove_atomic_temp(parent_dir, &temp_name);
            return Err(error);
        }
        if let Err(error) = file.write_all(content.as_bytes()) {
            remove_atomic_temp(parent_dir, &temp_name);
            return Err(error);
        }
        if let Err(error) = file.sync_all() {
            remove_atomic_temp(parent_dir, &temp_name);
            return Err(error);
        }
        if let Some(before_commit) = before_commit.as_deref_mut()
            && let Err(error) = before_commit()
        {
            remove_atomic_temp(parent_dir, &temp_name);
            return Err(error);
        }
        if let Some(identity) = metadata.identity {
            let replacement = file.metadata()?;
            let replacement_identity = (replacement.dev(), replacement.ino());
            drop(file);
            return commit_preserving_atomic_temp(
                parent_dir,
                &temp_name,
                file_name,
                identity,
                replacement_identity,
            );
        }
        drop(file);
        let renamed = match metadata.commit {
            AtomicCommit::Replace => {
                nix::fcntl::renameat(parent_dir, temp_name.as_str(), parent_dir, file_name)
            }
            AtomicCommit::NoReplace => nix::fcntl::renameat2(
                parent_dir,
                temp_name.as_str(),
                parent_dir,
                file_name,
                nix::fcntl::RenameFlags::RENAME_NOREPLACE,
            ),
        };
        if let Err(error) = renamed {
            remove_atomic_temp(parent_dir, &temp_name);
            return Err(nix_errno_to_io(error));
        }
        return parent_dir.sync_all();
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "cannot create unique temp file",
    ))
}

#[must_use]
pub fn generated_sibling_name(target: &str, kind: &str, attempt: u8) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!(".{target}.{kind}-{}-{nonce}-{attempt}", std::process::id())
}

#[must_use]
pub fn generated_sibling_target<'a>(name: &'a str, kind: &str) -> Option<&'a str> {
    let rest = name.strip_prefix('.')?;
    let marker = format!(".{kind}-");
    let (target, suffix) = rest.split_once(&marker)?;
    if target.is_empty() {
        return None;
    }
    let mut suffix = suffix.split('-');
    suffix.next()?.parse::<u32>().ok()?;
    suffix.next()?.parse::<u128>().ok()?;
    suffix.next()?.parse::<u8>().ok()?;
    suffix.next().is_none().then_some(target)
}

fn commit_preserving_atomic_temp(
    parent_dir: &fs::File,
    temp_name: &str,
    file_name: &str,
    expected: (u64, u64),
    replacement: (u64, u64),
) -> std::io::Result<()> {
    if support::plain::is_fuse(parent_dir)? {
        // CortexFS synthetic inodes are path-derived, so a cross-path exchange
        // cannot compare the temporary inode with the target inode. The mount
        // enforces owner-UID writes; same-UID writers are one security subject.
        if !atomic_target_matches(parent_dir, file_name, expected) {
            remove_atomic_temp(parent_dir, temp_name);
            return Err(std::io::Error::other("atomic replace target changed"));
        }
        if let Err(error) = nix::fcntl::renameat(parent_dir, temp_name, parent_dir, file_name) {
            remove_atomic_temp(parent_dir, temp_name);
            return Err(nix_errno_to_io(error));
        }
        return parent_dir.sync_all();
    }

    nix::fcntl::renameat2(
        parent_dir,
        temp_name,
        parent_dir,
        file_name,
        nix::fcntl::RenameFlags::RENAME_EXCHANGE,
    )
    .map_err(nix_errno_to_io)?;
    if atomic_target_matches(parent_dir, temp_name, expected) {
        remove_atomic_temp(parent_dir, temp_name);
        return parent_dir.sync_all();
    }
    if !atomic_target_matches(parent_dir, file_name, replacement) {
        return Err(std::io::Error::other(
            "atomic replace target changed during rollback",
        ));
    }
    nix::fcntl::renameat2(
        parent_dir,
        temp_name,
        parent_dir,
        file_name,
        nix::fcntl::RenameFlags::RENAME_EXCHANGE,
    )
    .map_err(nix_errno_to_io)?;
    remove_atomic_temp(parent_dir, temp_name);
    let _ignored = parent_dir.sync_all();
    Err(std::io::Error::other("atomic replace target changed"))
}

fn atomic_target_matches(parent_dir: &fs::File, file_name: &str, identity: (u64, u64)) -> bool {
    nix::sys::stat::fstatat(
        parent_dir,
        file_name,
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .is_ok_and(|stat| {
        nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
            .contains(nix::sys::stat::SFlag::S_IFREG)
            && (stat.st_dev, stat.st_ino) == identity
    })
}

fn remove_atomic_temp(parent_dir: &fs::File, temp_name: &str) {
    let _ignored = nix::unistd::unlinkat(
        parent_dir,
        temp_name,
        nix::unistd::UnlinkatFlags::NoRemoveDir,
    );
}

pub(crate) fn nix_errno_to_io(error: nix::errno::Errno) -> std::io::Error {
    std::io::Error::from(error)
}

pub(crate) fn unix_timestamp_text() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    format!("{seconds}\n")
}

pub(crate) fn tool_path_denial(error: ToolPathError) -> ToolExecutionDenial {
    match error {
        ToolPathError::InvalidName => ToolExecutionDenial::InvalidToolName,
        ToolPathError::CannotReadDirectory => ToolExecutionDenial::CannotReadToolPath,
    }
}

pub(crate) fn symlink_safe_metadata(path: &Path) -> std::io::Result<fs::Metadata> {
    let metadata = path_metadata_no_follow(path)?;
    if metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "symlink authority target refused",
        ));
    }
    Ok(metadata)
}

pub(crate) fn linux_identity_can_execute(
    metadata: &fs::Metadata,
    identity: &AgentUnixIdentity,
) -> bool {
    let mode = metadata.permissions().mode();
    if identity.uid() == 0 {
        return mode & 0o111 != 0;
    }
    if metadata.uid() == identity.uid() {
        return mode & 0o100 != 0;
    }
    if identity.is_in_group(metadata.gid()) {
        return mode & 0o010 != 0;
    }
    mode & 0o001 != 0
}

pub(crate) fn linux_identity_can_read(
    metadata: &fs::Metadata,
    identity: &AgentUnixIdentity,
) -> bool {
    linux_identity_has_mode(metadata, identity, 0o400, 0o040, 0o004, 0o444)
}

pub(crate) fn linux_identity_can_write(
    metadata: &fs::Metadata,
    identity: &AgentUnixIdentity,
) -> bool {
    linux_identity_has_mode(metadata, identity, 0o200, 0o020, 0o002, 0o222)
}

pub(crate) fn linux_identity_has_mode(
    metadata: &fs::Metadata,
    identity: &AgentUnixIdentity,
    owner_bit: u32,
    group_bit: u32,
    other_bit: u32,
    root_mask: u32,
) -> bool {
    let mode = metadata.permissions().mode();
    if identity.uid() == 0 {
        return mode & root_mask != 0;
    }
    if metadata.uid() == identity.uid() {
        return mode & owner_bit != 0;
    }
    if identity.is_in_group(metadata.gid()) {
        return mode & group_bit != 0;
    }
    mode & other_bit != 0
}

pub(crate) fn most_specific_mount_for_path<'a>(
    mount_table: &'a MountTable,
    path: &Path,
) -> Option<&'a MountEntry> {
    mount_table
        .entries()
        .iter()
        .filter(|entry| path.starts_with(Path::new(entry.target())))
        .max_by_key(|entry| Path::new(entry.target()).components().count())
}

pub(crate) fn is_stable_shared_mount_for(mount: &MountEntry, shared_name: &str) -> bool {
    is_stable_shared_path_for(Path::new(mount.source()), shared_name)
        || is_stable_shared_path_for(Path::new(mount.target()), shared_name)
}

pub(crate) fn is_stable_shared_path_for(path: &Path, shared_name: &str) -> bool {
    let mut parts = path.components().filter_map(|component| match component {
        std::path::Component::Normal(value) => value.to_str(),
        std::path::Component::RootDir
        | std::path::Component::CurDir
        | std::path::Component::ParentDir
        | std::path::Component::Prefix(_) => None,
    });

    match (parts.next(), parts.next(), parts.next()) {
        (Some("ctx"), Some("shared"), Some(name)) | (Some("shared"), Some(name), _) => {
            name == shared_name
        }
        _ => false,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MountedSessionPath {
    Private {
        uid: u32,
        agent: String,
        session: String,
    },
    Shared {
        shared: String,
        agent: String,
        session: String,
    },
}

impl MountedSessionPath {
    pub(crate) fn session_name(&self) -> &str {
        match *self {
            Self::Private { ref session, .. } | Self::Shared { ref session, .. } => session,
        }
    }

    pub(crate) fn shared_name(&self) -> Option<&str> {
        match *self {
            Self::Shared { ref shared, .. } => Some(shared),
            Self::Private { .. } => None,
        }
    }

    pub(crate) fn home_uid_allows(&self, identity: &AgentUnixIdentity) -> bool {
        match *self {
            Self::Private { uid, .. } => uid == identity.uid(),
            Self::Shared { .. } => true,
        }
    }
}

pub(crate) fn mounted_session_path(mount: &MountEntry, path: &Path) -> Option<MountedSessionPath> {
    let stable = mounted_stable_path(mount, path)?;
    parse_mounted_session_path(&stable)
}

pub(crate) fn mounted_stable_path(mount: &MountEntry, path: &Path) -> Option<PathBuf> {
    let target = Path::new(mount.target());
    if let Ok(relative) = path.strip_prefix(target) {
        return Some(Path::new(mount.source()).join(relative));
    }
    path.starts_with(Path::new(mount.source()))
        .then(|| path.to_path_buf())
}

pub(crate) fn parse_mounted_session_path(path: &Path) -> Option<MountedSessionPath> {
    let parts = stable_path_parts(path)?;
    match *parts.as_slice() {
        ["ctx", "home", uid, "agent", agent, "session", session, ..]
        | ["home", uid, "agent", agent, "session", session, ..] => {
            let uid = uid.parse::<u32>().ok()?;
            (is_object_name(agent) && is_object_name(session)).then(|| {
                MountedSessionPath::Private {
                    uid,
                    agent: (*agent).to_owned(),
                    session: (*session).to_owned(),
                }
            })
        }
        [
            "ctx",
            "shared",
            shared,
            "agent",
            agent,
            "session",
            session,
            ..,
        ]
        | ["shared", shared, "agent", agent, "session", session, ..] => (is_object_name(shared)
            && is_object_name(agent)
            && is_object_name(session))
        .then(|| MountedSessionPath::Shared {
            shared: (*shared).to_owned(),
            agent: (*agent).to_owned(),
            session: (*session).to_owned(),
        }),
        _ => None,
    }
}

pub(crate) fn stable_path_parts(path: &Path) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(value) => parts.push(value.to_str()?),
            std::path::Component::RootDir => {}
            std::path::Component::CurDir
            | std::path::Component::ParentDir
            | std::path::Component::Prefix(_) => return None,
        }
    }
    Some(parts)
}
