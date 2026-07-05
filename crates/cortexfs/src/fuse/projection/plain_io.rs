use crate::plain_fs::{
    create_plain_dir as create_fuse_v1_plain_dir,
    open_plain_directory as open_fuse_v1_plain_directory,
    open_plain_file as open_fuse_v1_plain_file,
    path_metadata_no_follow as fuse_v1_plain_path_metadata,
    plain_file_name as fuse_v1_plain_file_name,
    read_small_text_file as read_fuse_v1_small_text_file,
    read_symlink_target as read_fuse_v1_symlink_target,
};

fn fuse_file_type_from_mode(mode: libc::mode_t) -> FuseV1FileType {
    match mode & libc::S_IFMT {
        libc::S_IFDIR => FuseV1FileType::Directory,
        libc::S_IFREG => FuseV1FileType::Regular,
        libc::S_IFLNK => FuseV1FileType::Symlink,
        libc::S_IFSOCK => FuseV1FileType::Socket,
        _ => FuseV1FileType::Other,
    }
}

fn fuse_v1_plain_dir_exists(path: &Path) -> Result<bool, FuseV1Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(true),
        Ok(_metadata) => Err(FuseV1Error::Io),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_error) => Err(FuseV1Error::Io),
    }
}
