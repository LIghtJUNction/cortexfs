#![allow(
    clippy::redundant_pub_crate,
    reason = "plain_fs is a private module shared by sibling modules without becoming public API"
)]

use std::fs;

use std::io::{Read, Result};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

use nix::libc;

pub(crate) fn read_symlink_target(path: &Path) -> Result<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_dir = open_plain_directory(parent)?;
    let file_name = plain_file_name(path)?;
    nix::fcntl::readlinkat(&parent_dir, file_name)
        .map(PathBuf::from)
        .map_err(std::io::Error::from)
}

pub(crate) fn read_small_text_file(path: &Path, max_bytes: u64) -> Result<String> {
    let file = open_plain_file(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "path is not a regular file",
        ));
    }
    if metadata.len() > max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file exceeds read limit",
        ));
    }
    read_file_to_string(file, metadata.len())
}

#[doc(hidden)]
pub fn read_small_text_file_at(
    directory: &fs::File,
    name: &str,
    max_bytes: u64,
    invalid_message: &'static str,
) -> Result<String> {
    let file_fd = nix::fcntl::openat(
        directory,
        name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    let file = fs::File::from(file_fd);
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            invalid_message,
        ));
    }
    read_file_to_string(file, metadata.len())
}

pub(crate) fn path_metadata_no_follow(path: &Path) -> Result<fs::Metadata> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_dir = open_plain_directory(parent)?;
    let file_name = plain_file_name(path)?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_PATH | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    fs::File::from(file_fd).metadata()
}

pub(crate) fn open_plain_file(path: &Path) -> Result<fs::File> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_dir = open_plain_directory(parent)?;
    let file_name = plain_file_name(path)?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    Ok(fs::File::from(file_fd))
}

pub(crate) fn plain_file_name(path: &Path) -> Result<&str> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid file name"))
}

pub(crate) fn proc_fd_path(file: &fs::File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

/// Messages and error kinds used while creating a plain (no-follow) directory tree.
#[derive(Clone, Copy, Debug)]
pub struct CreatePlainDirMessages {
    pub mode: u32,
    pub existing_not_dir_kind: std::io::ErrorKind,
    pub existing_not_dir_message: &'static str,
    pub contains_non_dir_kind: std::io::ErrorKind,
    pub contains_non_dir_message: &'static str,
    pub invalid_name_message: &'static str,
}

impl CreatePlainDirMessages {
    /// Library defaults used by bootstrap / FUSE paths.
    #[must_use]
    pub const fn library_defaults() -> Self {
        Self {
            mode: 0o755,
            existing_not_dir_kind: std::io::ErrorKind::InvalidInput,
            existing_not_dir_message: "path is not a plain directory",
            contains_non_dir_kind: std::io::ErrorKind::InvalidInput,
            contains_non_dir_message: "path contains a non-directory entry",
            invalid_name_message: "invalid directory",
        }
    }
}

pub(crate) fn create_plain_dir(path: &Path) -> Result<()> {
    create_plain_dir_with(path, CreatePlainDirMessages::library_defaults())
}

/// Creates exactly one plain directory and fails when the final entry exists.
pub fn create_plain_dir_exclusive(path: &Path, mode: u32) -> Result<fs::File> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let parent_dir = open_plain_directory(parent)?;
    let name = plain_file_name(path)?;
    nix::sys::stat::mkdirat(
        &parent_dir,
        name,
        nix::sys::stat::Mode::from_bits_truncate(mode & 0o7777),
    )
    .map_err(std::io::Error::from)?;
    let created = nix::fcntl::openat(
        &parent_dir,
        name,
        nix::fcntl::OFlag::O_DIRECTORY
            | nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map(fs::File::from)
    .map_err(std::io::Error::from);
    let created = match created {
        Ok(created) => created,
        Err(error) => {
            let _ignored = remove_plain_dir_at(&parent_dir, name);
            return Err(error);
        }
    };
    if let Err(error) = created.sync_all().and_then(|()| parent_dir.sync_all()) {
        drop(created);
        let _ignored = remove_plain_dir_at(&parent_dir, name);
        return Err(error);
    }
    Ok(created)
}

pub(crate) fn remove_plain_dir(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let parent_dir = open_plain_directory(parent)?;
    let name = plain_file_name(path)?;
    remove_plain_dir_at(&parent_dir, name)?;
    parent_dir.sync_all()
}

fn remove_plain_dir_at(parent: &fs::File, name: &str) -> Result<()> {
    nix::unistd::unlinkat(parent, name, nix::unistd::UnlinkatFlags::RemoveDir)
        .map_err(std::io::Error::from)
}

/// Ensures that `path` is a no-follow Unix socket placeholder.
///
/// Returns `true` only when this call created the socket inode.
pub fn ensure_socket_placeholder(path: &Path, mode: u32) -> Result<bool> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    create_plain_dir(parent)?;
    let parent_dir = open_plain_directory(parent)?;
    let name = plain_file_name(path)?;
    match nix::sys::stat::fstatat(&parent_dir, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(stat)
            if nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
                .contains(nix::sys::stat::SFlag::S_IFSOCK) =>
        {
            set_socket_mode(&parent_dir, name, mode)?;
            return Ok(false);
        }
        Ok(stat)
            if nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
                .contains(nix::sys::stat::SFlag::S_IFLNK) =>
        {
            match nix::sys::stat::fstatat(&parent_dir, name, nix::fcntl::AtFlags::empty()) {
                Ok(target)
                    if nix::sys::stat::SFlag::from_bits_truncate(target.st_mode)
                        .contains(nix::sys::stat::SFlag::S_IFSOCK) =>
                {
                    return Ok(false);
                }
                Err(nix::errno::Errno::ENOENT) => {
                    nix::unistd::unlinkat(
                        &parent_dir,
                        name,
                        nix::unistd::UnlinkatFlags::NoRemoveDir,
                    )
                    .map_err(std::io::Error::from)?;
                }
                Ok(_) | Err(_) => {
                    return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists));
                }
            }
        }
        Ok(_stat) => return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists)),
        Err(nix::errno::Errno::ENOENT) => {}
        Err(error) => return Err(std::io::Error::from(error)),
    }
    UnixListener::bind(path)?;
    if let Err(error) = set_socket_mode(&parent_dir, name, mode) {
        let _ignored =
            nix::unistd::unlinkat(&parent_dir, name, nix::unistd::UnlinkatFlags::NoRemoveDir);
        return Err(error);
    }
    Ok(true)
}

fn set_socket_mode(parent: &fs::File, name: &str, mode: u32) -> Result<()> {
    nix::sys::stat::fchmodat(
        parent,
        name,
        nix::sys::stat::Mode::from_bits_truncate(mode & 0o7777),
        nix::sys::stat::FchmodatFlags::NoFollowSymlink,
    )
    .map_err(std::io::Error::from)
}

/// Creates a plain directory tree with custom mode and error messages.
pub fn create_plain_dir_with(path: &Path, messages: CreatePlainDirMessages) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        return if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            sync_plain_dir(path)
        } else {
            Err(std::io::Error::new(
                messages.existing_not_dir_kind,
                messages.existing_not_dir_message,
            ))
        };
    }

    let mut missing = Vec::new();
    let mut cursor = Some(path);
    while let Some(current) = cursor {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() => {
                return Err(std::io::Error::new(
                    messages.contains_non_dir_kind,
                    messages.contains_non_dir_message,
                ));
            }
            Ok(_metadata) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                cursor = current.parent();
            }
            Err(error) => return Err(error),
        }
    }

    let existing_parent = missing
        .last()
        .and_then(|path| path.parent())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                messages.invalid_name_message,
            )
        })?;
    let mut parent_dir = open_plain_directory(existing_parent)?;
    for directory in missing.iter().rev() {
        let name = plain_file_name(directory).map_err(|_error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                messages.invalid_name_message,
            )
        })?;
        nix::sys::stat::mkdirat(
            &parent_dir,
            name,
            nix::sys::stat::Mode::from_bits_truncate(messages.mode),
        )
        .map_err(std::io::Error::from)?;
        parent_dir.sync_all()?;
        let child = nix::fcntl::openat(
            &parent_dir,
            name,
            nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )
        .map_err(std::io::Error::from)?;
        parent_dir = fs::File::from(child);
        parent_dir.sync_all()?;
    }
    Ok(())
}

pub(crate) fn sync_plain_dir(path: &Path) -> Result<()> {
    open_plain_directory(path)?.sync_all()
}

#[doc(hidden)]
pub fn open_plain_directory(path: &Path) -> Result<fs::File> {
    let mut directory = if path.is_absolute() {
        open_single_plain_directory(Path::new("/"))?
    } else {
        open_single_plain_directory(Path::new("."))?
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid directory name")
                })?;
                let next = nix::fcntl::openat(
                    &directory,
                    name,
                    nix::fcntl::OFlag::O_DIRECTORY
                        | nix::fcntl::OFlag::O_RDONLY
                        | nix::fcntl::OFlag::O_NOFOLLOW
                        | nix::fcntl::OFlag::O_CLOEXEC,
                    nix::sys::stat::Mode::empty(),
                )
                .map_err(std::io::Error::from)?;
                directory = fs::File::from(next);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "directory path contains unsupported components",
                ));
            }
        }
    }
    Ok(directory)
}

pub(crate) fn open_single_plain_directory(path: &Path) -> Result<fs::File> {
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path is not a plain directory",
        ));
    }
    Ok(directory)
}

pub(crate) fn read_file_to_string(mut file: fs::File, len: u64) -> Result<String> {
    let len = usize::try_from(len).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("file is too large to read: {error}"),
        )
    })?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.utf8_error()))
}
