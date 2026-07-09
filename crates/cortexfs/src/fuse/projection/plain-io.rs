use super::*;

pub(crate) fn fuse_file_type_from_mode(mode: libc::mode_t) -> FuseV1FileType {
    match mode & libc::S_IFMT {
        libc::S_IFDIR => FuseV1FileType::Directory,
        libc::S_IFREG => FuseV1FileType::Regular,
        libc::S_IFLNK => FuseV1FileType::Symlink,
        libc::S_IFSOCK => FuseV1FileType::Socket,
        _ => FuseV1FileType::Other,
    }
}

pub(crate) fn fuse_v1_plain_dir_exists(path: &Path) -> Result<bool, FuseV1Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(true),
        Ok(_metadata) => Err(FuseV1Error::Io),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_error) => Err(FuseV1Error::Io),
    }
}
