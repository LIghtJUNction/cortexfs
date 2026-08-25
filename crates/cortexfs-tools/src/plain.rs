use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

pub(crate) fn plain_file_name(path: &Path) -> io::Result<&str> {
    path.file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid file name"))
}

pub(crate) fn proc_fd_path(file: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

pub(crate) fn open_plain_directory(path: &Path) -> io::Result<File> {
    let mut directory = open_single_plain_directory(if path.is_absolute() {
        Path::new("/")
    } else {
        Path::new(".")
    })?;
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => directory = open_directory_at(&directory, name)?,
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "directory path contains unsupported components",
                ));
            }
        }
    }
    Ok(directory)
}

fn open_single_plain_directory(path: &Path) -> io::Result<File> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW)
        .open(path)?;
    if directory.metadata()?.is_dir() {
        Ok(directory)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a plain directory",
        ))
    }
}

fn open_directory_at(parent: &File, name: &OsStr) -> io::Result<File> {
    nix::fcntl::openat(
        parent,
        name,
        nix::fcntl::OFlag::O_DIRECTORY
            | nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map(File::from)
    .map_err(io::Error::from)
}

pub(crate) fn open_plain_file(path: &Path) -> io::Result<File> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let directory = open_plain_directory(parent)?;
    let name = plain_file_name(path)?;
    nix::fcntl::openat(
        &directory,
        name,
        nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_NONBLOCK
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map(File::from)
    .map_err(io::Error::from)
}

pub(crate) fn path_metadata_no_follow(path: &Path) -> io::Result<fs::Metadata> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let directory = open_plain_directory(parent)?;
    let name = plain_file_name(path)?;
    let file = nix::fcntl::openat(
        &directory,
        name,
        nix::fcntl::OFlag::O_PATH | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map(File::from)
    .map_err(io::Error::from)?;
    file.metadata()
}

pub(crate) fn create_exclusive_file_at(parent: &File, name: &str, mode: u32) -> nix::Result<File> {
    nix::fcntl::openat(
        parent,
        name,
        nix::fcntl::OFlag::O_WRONLY
            | nix::fcntl::OFlag::O_CREAT
            | nix::fcntl::OFlag::O_EXCL
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::from_bits_truncate(mode),
    )
    .map(File::from)
}
