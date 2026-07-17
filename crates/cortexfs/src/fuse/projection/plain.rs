use super::*;

pub(crate) fn fuse_file_type_from_mode(mode: libc::mode_t) -> FuseFileType {
    match mode & libc::S_IFMT {
        libc::S_IFDIR => FuseFileType::Directory,
        libc::S_IFREG => FuseFileType::Regular,
        libc::S_IFLNK => FuseFileType::Symlink,
        libc::S_IFSOCK => FuseFileType::Socket,
        _ => FuseFileType::Other,
    }
}

pub(crate) fn fuse_plain_dir_exists(path: &Path) -> Result<bool, FuseError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(true),
        Ok(_metadata) => Err(FuseError::Io),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_error) => Err(FuseError::Io),
    }
}
