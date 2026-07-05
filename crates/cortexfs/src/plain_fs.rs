#![allow(
    clippy::redundant_pub_crate,
    reason = "plain_fs is a private module shared by sibling modules without becoming public API"
)]

use std::fs;
use std::io::{Read, Result};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
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

pub(crate) fn create_plain_dir(path: &Path) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        return if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            sync_plain_dir(path)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "path is not a plain directory",
            ))
        };
    }

    let mut missing = Vec::new();
    let mut cursor = Some(path);
    while let Some(current) = cursor {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "path contains a non-directory entry",
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
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid directory"))?;
    let mut parent_dir = open_plain_directory(existing_parent)?;
    for directory in missing.iter().rev() {
        let name = plain_file_name(directory)?;
        nix::sys::stat::mkdirat(
            &parent_dir,
            name,
            nix::sys::stat::Mode::from_bits_truncate(0o755),
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

fn open_single_plain_directory(path: &Path) -> Result<fs::File> {
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

fn read_file_to_string(mut file: fs::File, len: u64) -> Result<String> {
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
