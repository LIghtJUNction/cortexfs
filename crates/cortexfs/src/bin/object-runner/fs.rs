use crate::*;

pub(crate) fn is_regular_file_no_follow(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(parent_dir) = open_plain_directory(parent) else {
        return false;
    };
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Ok(file_fd) = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_PATH | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    ) else {
        return false;
    };
    fs::File::from(file_fd)
        .metadata()
        .is_ok_and(|metadata| metadata.is_file())
}
