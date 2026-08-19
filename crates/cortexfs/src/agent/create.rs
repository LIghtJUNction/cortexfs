use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{ObjectClass, is_object_name};

/// Files created for one ordinary agent object.
#[derive(Debug)]
pub struct AgentCreatePaths {
    executable: PathBuf,
    control: PathBuf,
    socket: PathBuf,
    home: PathBuf,
    owned: Vec<OwnedPath>,
}

#[derive(Debug)]
struct OwnedPath {
    path: PathBuf,
    dev: u64,
    ino: u64,
    directory: bool,
}

static ROLLBACK_ID: AtomicU64 = AtomicU64::new(0);

impl AgentCreatePaths {
    /// Derives the ordinary object, control, socket, and private-home paths.
    #[must_use]
    pub(crate) fn new(root: &Path, uid: &str, name: &str) -> Self {
        Self {
            executable: cortexfs_paths::agent_path(root, name),
            control: cortexfs_paths::agent_control_path(root, name),
            socket: cortexfs_paths::agent_socket_path(root, name),
            home: cortexfs_paths::agent_home_path(root, uid, name),
            owned: Vec::new(),
        }
    }

    fn own(&mut self, path: &Path) -> Result<(), AgentCreateError> {
        let metadata =
            fs::symlink_metadata(path).map_err(|_error| AgentCreateError::CannotCreate)?;
        self.owned.push(OwnedPath {
            path: path.to_owned(),
            dev: metadata.dev(),
            ino: metadata.ino(),
            directory: metadata.is_dir(),
        });
        Ok(())
    }

    fn own_dir(&mut self, path: PathBuf, file: &fs::File) -> Result<(), AgentCreateError> {
        let metadata = file
            .metadata()
            .map_err(|_error| AgentCreateError::CannotCreate)?;
        self.owned.push(OwnedPath {
            path,
            dev: metadata.dev(),
            ino: metadata.ino(),
            directory: true,
        });
        Ok(())
    }

    pub(crate) fn own_session_layout(&mut self, receipts: crate::SessionLayoutReceipts) {
        self.owned.extend(
            receipts
                .into_entries()
                .into_iter()
                .map(|receipt| OwnedPath {
                    path: receipt.path,
                    dev: receipt.dev,
                    ino: receipt.ino,
                    directory: receipt.directory,
                }),
        );
    }
}

fn own_control_children(
    paths: &mut AgentCreatePaths,
    directory_path: &Path,
    directory: &fs::File,
) -> Result<(), AgentCreateError> {
    let mut count = 0_usize;
    own_control_children_bounded(paths, directory_path, directory, 0, &mut count)
}

fn own_control_children_bounded(
    paths: &mut AgentCreatePaths,
    directory_path: &Path,
    directory: &fs::File,
    depth: usize,
    count: &mut usize,
) -> Result<(), AgentCreateError> {
    const MAX_CONTROL_DEPTH: usize = 8;
    const MAX_CONTROL_ENTRIES: usize = 256;
    if depth > MAX_CONTROL_DEPTH {
        return Err(AgentCreateError::CannotCreate);
    }
    let mut names = fs::read_dir(crate::support::plain::proc_fd_path(directory))
        .map_err(|_error| AgentCreateError::CannotCreate)?
        .map(|entry| {
            entry
                .map(|entry| entry.file_name())
                .map_err(|_error| AgentCreateError::CannotCreate)
        })
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    for name in names {
        *count = count.saturating_add(1);
        if *count > MAX_CONTROL_ENTRIES {
            return Err(AgentCreateError::CannotCreate);
        }
        let name = name.to_str().ok_or(AgentCreateError::CannotCreate)?;
        let fd = nix::fcntl::openat(
            directory,
            name,
            nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC
                | nix::fcntl::OFlag::O_NONBLOCK,
            nix::sys::stat::Mode::empty(),
        )
        .map(fs::File::from)
        .map_err(|_error| AgentCreateError::CannotCreate)?;
        let metadata = fd
            .metadata()
            .map_err(|_error| AgentCreateError::CannotCreate)?;
        let rebound =
            nix::sys::stat::fstatat(directory, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
                .map_err(|_error| AgentCreateError::CannotCreate)?;
        let kind = nix::sys::stat::SFlag::from_bits_truncate(rebound.st_mode);
        if (rebound.st_dev, rebound.st_ino) != (metadata.dev(), metadata.ino())
            || metadata.is_dir() != kind.contains(nix::sys::stat::SFlag::S_IFDIR)
            || metadata.is_file() != kind.contains(nix::sys::stat::SFlag::S_IFREG)
        {
            return Err(AgentCreateError::CannotCreate);
        }
        let path = directory_path.join(name);
        if metadata.is_dir() {
            paths.own_dir(path.clone(), &fd)?;
            own_control_children_bounded(paths, &path, &fd, depth + 1, count)?;
        } else if metadata.is_file() {
            paths.owned.push(OwnedPath {
                path,
                dev: metadata.dev(),
                ino: metadata.ino(),
                directory: false,
            });
        } else {
            return Err(AgentCreateError::CannotCreate);
        }
    }
    Ok(())
}

#[expect(
    dead_code,
    reason = "used by the withheld agent.create tool transaction"
)]
pub(crate) fn rollback_agent_files(
    mut receipt: AgentCreatePaths,
) -> Result<(), AgentRollbackError> {
    rollback(&mut receipt, |_stage, _path| {})
}

pub(crate) fn rollback_session_layout(
    receipts: crate::SessionLayoutReceipts,
) -> Result<(), AgentRollbackError> {
    let mut paths = AgentCreatePaths::new(Path::new("/"), "0", "receipt");
    paths.own_session_layout(receipts);
    rollback(&mut paths, |_stage, _path| {})
}

#[cfg(test)]
pub(crate) fn rollback_agent_files_with_hook(
    mut receipt: AgentCreatePaths,
    hook: impl FnMut(AgentRollbackStage, &Path),
) -> Result<(), AgentRollbackError> {
    rollback(&mut receipt, hook)
}

/// Failure while transactionally materialising an ordinary child agent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentCreateError {
    InvalidInput,
    AlreadyExists,
    CannotCreate,
    RollbackConflict(AgentRollbackConflict),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AgentRollbackError {
    Conflict(AgentRollbackConflict),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentRollbackConflict {
    /// Original ABI path whose recorded inode could not be safely rolled back.
    pub original: PathBuf,
    /// Isolated receipt path when the inode was already moved out of the ABI name.
    pub quarantine: Option<PathBuf>,
    /// Expected device number recorded by the transaction.
    pub dev: u64,
    /// Expected inode number recorded by the transaction.
    pub ino: u64,
    /// Stable rollback stage at which ownership proof failed.
    pub stage: &'static str,
}

/// Formats a stable rollback-conflict diagnostic for CLI and tool errors.
#[must_use]
pub fn format_agent_rollback_conflict(conflict: &AgentRollbackConflict) -> String {
    format!(
        "original={} quarantine={} dev={} ino={} stage={}",
        conflict.original.display(),
        conflict
            .quarantine
            .as_deref()
            .map_or_else(|| "-".to_owned(), |path| path.display().to_string()),
        conflict.dev,
        conflict.ino,
        conflict.stage
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentRollbackStage {
    Quarantined,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentCreateStage {
    Control,
    Controls,
    Wrapper,
    Executable,
    Socket,
    Home,
    HomeBound,
    Skeleton,
    SessionBound,
}

/// Materialises an ordinary agent and rolls back paths owned by this transaction.
pub fn create_agent_files(
    root: &Path,
    uid: &str,
    name: &str,
    executable: &str,
    overrides: &[(&str, &str)],
) -> Result<AgentCreatePaths, AgentCreateError> {
    create_agent_files_with_hook(root, uid, name, executable, overrides, |_stage| Ok(()))
}

pub(crate) fn create_agent_files_with_hook(
    root: &Path,
    uid: &str,
    name: &str,
    executable: &str,
    overrides: &[(&str, &str)],
    mut hook: impl FnMut(AgentCreateStage) -> Result<(), AgentCreateError>,
) -> Result<AgentCreatePaths, AgentCreateError> {
    let uid_number = uid
        .parse::<u32>()
        .map_err(|_error| AgentCreateError::InvalidInput)?;
    let gid_number = overrides
        .iter()
        .find_map(|entry| (entry.0 == "gid").then_some(entry.1))
        .unwrap_or(uid)
        .parse::<u32>()
        .map_err(|_error| AgentCreateError::InvalidInput)?;
    if !is_object_name(name) || executable.contains('\0') {
        return Err(AgentCreateError::InvalidInput);
    }
    let mut paths = AgentCreatePaths::new(root, uid, name);
    for path in [
        &paths.executable,
        &paths.control,
        &paths.socket,
        &paths.home,
    ] {
        match fs::symlink_metadata(path) {
            Ok(_) => return Err(AgentCreateError::AlreadyExists),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(AgentCreateError::CannotCreate),
        }
    }
    crate::support::plain::create_plain_dir(&cortexfs_paths::agent_root_path(root))
        .map_err(|_error| AgentCreateError::CannotCreate)?;
    let home_root = cortexfs_paths::home_root_path(root);
    let user_home = home_root.join(uid);
    let agent_home = cortexfs_paths::home_agent_root_from_home_path(&user_home);
    for parent in [&home_root, &user_home, &agent_home] {
        crate::support::plain::create_plain_dir(parent)
            .map_err(|_error| AgentCreateError::CannotCreate)?;
    }

    let result = (|| {
        hook(AgentCreateStage::Control)?;
        crate::support::plain::create_plain_dir_exclusive(&paths.control, 0o755)
            .map_err(|_error| AgentCreateError::CannotCreate)?;
        let control = paths.control.clone();
        paths.own(&control)?;
        hook(AgentCreateStage::Controls)?;
        crate::install_object_control_files(&paths.control, ObjectClass::Agent, name, overrides)
            .map_err(|_error| AgentCreateError::CannotCreate)?;
        hook(AgentCreateStage::Wrapper)?;
        crate::ensure_object_hook_dirs(&paths.control)
            .map_err(|_error| AgentCreateError::CannotCreate)?;
        let control_fd = crate::support::plain::open_plain_directory(&paths.control)
            .map_err(|_error| AgentCreateError::CannotCreate)?;
        let control_path = paths.control.clone();
        own_control_children(&mut paths, &control_path, &control_fd)?;
        hook(AgentCreateStage::Executable)?;
        crate::atomic_create_text_with_mode(&paths.executable, executable, 0o755)
            .map_err(|_error| AgentCreateError::CannotCreate)?;
        let executable_path = paths.executable.clone();
        paths.own(&executable_path)?;
        hook(AgentCreateStage::Socket)?;
        let socket_created = crate::support::plain::ensure_socket_placeholder(&paths.socket, 0o777)
            .map_err(|_error| AgentCreateError::CannotCreate)?;
        if !socket_created {
            return Err(AgentCreateError::AlreadyExists);
        }
        let socket = paths.socket.clone();
        paths.own(&socket)?;
        hook(AgentCreateStage::Home)?;
        let home_fd = crate::support::plain::create_plain_dir_exclusive(&paths.home, 0o755)
            .map_err(|_error| AgentCreateError::CannotCreate)?;
        let home = paths.home.clone();
        paths.own_dir(home, &home_fd)?;
        chown_home_entry(&home_fd, uid_number, gid_number)?;
        hook(AgentCreateStage::HomeBound)?;
        hook(AgentCreateStage::Skeleton)?;
        for name in ["root", "data", "cache", "log"] {
            let file = crate::support::plain::create_plain_dir_at(&home_fd, name, 0o755)
                .map_err(|_error| AgentCreateError::CannotCreate)?;
            paths.own_dir(paths.home.join(name), &file)?;
            chown_home_entry(&file, uid_number, gid_number)?;
        }
        let session_fd = crate::support::plain::create_plain_dir_at(&home_fd, "session", 0o755)
            .map_err(|_error| AgentCreateError::CannotCreate)?;
        paths.own_dir(paths.home.join("session"), &session_fd)?;
        chown_home_entry(&session_fd, uid_number, gid_number)?;
        hook(AgentCreateStage::SessionBound)?;
        let index_fd = crate::support::plain::create_plain_dir_at(&session_fd, "index", 0o755)
            .map_err(|_error| AgentCreateError::CannotCreate)?;
        paths.own_dir(paths.home.join("session/index"), &index_fd)?;
        chown_home_entry(&index_fd, uid_number, gid_number)?;
        for name in [
            "by-cwd",
            "by-hash",
            "by-uuid",
            cortexfs_paths::SESSION_CHANNEL_INDEX,
        ] {
            let file = crate::support::plain::create_plain_dir_at(&index_fd, name, 0o755)
                .map_err(|_error| AgentCreateError::CannotCreate)?;
            paths.own_dir(paths.home.join("session/index").join(name), &file)?;
            chown_home_entry(&file, uid_number, gid_number)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        return match rollback(&mut paths, |_stage, _path| {}) {
            Ok(()) => Err(error),
            Err(AgentRollbackError::Conflict(conflict)) => {
                Err(AgentCreateError::RollbackConflict(conflict))
            }
        };
    }
    Ok(paths)
}

pub(crate) fn chown_home_entry(
    file: &fs::File,
    uid: u32,
    gid: u32,
) -> Result<(), AgentCreateError> {
    let metadata = file
        .metadata()
        .map_err(|_error| AgentCreateError::CannotCreate)?;
    if metadata.uid() == uid && metadata.gid() == gid {
        return Ok(());
    }
    nix::unistd::fchown(
        file,
        Some(nix::unistd::Uid::from_raw(uid)),
        Some(nix::unistd::Gid::from_raw(gid)),
    )
    .map_err(|_error| AgentCreateError::CannotCreate)
}

fn rollback(
    paths: &mut AgentCreatePaths,
    mut hook: impl FnMut(AgentRollbackStage, &Path),
) -> Result<(), AgentRollbackError> {
    let mut result = Ok(());
    let mut conflicted_paths = Vec::new();
    for owned in paths.owned.drain(..).rev() {
        if conflicted_paths
            .iter()
            .any(|path: &PathBuf| path.starts_with(&owned.path))
        {
            continue;
        }
        if let Err(error) = rollback_owned(&owned, &mut hook) {
            let AgentRollbackError::Conflict(ref conflict) = error;
            conflicted_paths.push(conflict.original.clone());
            if result.is_ok() {
                result = Err(error);
            }
        }
    }
    result
}

fn rollback_owned(
    owned: &OwnedPath,
    hook: &mut impl FnMut(AgentRollbackStage, &Path),
) -> Result<(), AgentRollbackError> {
    let Some(parent) = owned.path.parent() else {
        return Err(rollback_conflict(owned, None, "parent"));
    };
    let Ok(parent_dir) = crate::support::plain::open_plain_directory(parent) else {
        return Err(rollback_conflict(owned, None, "parent-open"));
    };
    let Some(name) = owned.path.file_name().and_then(|name| name.to_str()) else {
        return Err(rollback_conflict(owned, None, "name"));
    };
    let original_matches =
        nix::sys::stat::fstatat(&parent_dir, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
            .is_ok_and(|stat| (stat.st_dev, stat.st_ino) == (owned.dev, owned.ino));
    if !original_matches {
        return Err(rollback_conflict(owned, None, "original-precheck"));
    }
    let quarantine = format!(
        ".ctx-rollback-{}-{}",
        std::process::id(),
        ROLLBACK_ID.fetch_add(1, Ordering::Relaxed)
    );
    if nix::fcntl::renameat2(
        &parent_dir,
        name,
        &parent_dir,
        quarantine.as_str(),
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    )
    .is_err()
    {
        return Err(rollback_conflict(
            owned,
            Some(parent.join(&quarantine)),
            "rename",
        ));
    }
    hook(AgentRollbackStage::Quarantined, &owned.path);
    let matches = nix::sys::stat::fstatat(
        &parent_dir,
        quarantine.as_str(),
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .is_ok_and(|stat| {
        let kind = nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode);
        (stat.st_dev, stat.st_ino) == (owned.dev, owned.ino)
            && if owned.directory {
                kind.contains(nix::sys::stat::SFlag::S_IFDIR)
            } else {
                kind.contains(nix::sys::stat::SFlag::S_IFREG)
                    || kind.contains(nix::sys::stat::SFlag::S_IFSOCK)
            }
    });
    if !matches {
        let _ignored = nix::fcntl::renameat2(
            &parent_dir,
            quarantine.as_str(),
            &parent_dir,
            name,
            nix::fcntl::RenameFlags::RENAME_NOREPLACE,
        );
        return Err(rollback_conflict(
            owned,
            Some(parent.join(&quarantine)),
            "quarantine-postcheck",
        ));
    }
    if nix::sys::stat::fstatat(&parent_dir, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW).is_ok()
    {
        return Err(rollback_conflict(
            owned,
            Some(parent.join(&quarantine)),
            "original-recreated",
        ));
    }
    let removal = if owned.directory {
        nix::unistd::unlinkat(
            &parent_dir,
            quarantine.as_str(),
            nix::unistd::UnlinkatFlags::RemoveDir,
        )
    } else {
        nix::unistd::unlinkat(
            &parent_dir,
            quarantine.as_str(),
            nix::unistd::UnlinkatFlags::NoRemoveDir,
        )
    };
    if removal.is_err() {
        return Err(rollback_conflict(
            owned,
            Some(parent.join(&quarantine)),
            if owned.directory { "rmdir" } else { "unlink" },
        ));
    }
    Ok(())
}

fn rollback_conflict(
    owned: &OwnedPath,
    quarantine: Option<PathBuf>,
    stage: &'static str,
) -> AgentRollbackError {
    AgentRollbackError::Conflict(AgentRollbackConflict {
        original: owned.path.clone(),
        quarantine,
        dev: owned.dev,
        ino: owned.ino,
        stage,
    })
}
