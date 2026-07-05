use crate::plain_fs::{
    open_plain_directory as open_object_layout_plain_directory,
    path_metadata_no_follow as object_layout_plain_path_metadata,
    plain_file_name as object_layout_plain_file_name,
    read_small_text_file as read_object_layout_small_text_file,
};

const MAX_OBJECT_LAYOUT_CONTROL_BYTES: u64 = 64 * 1024;

fn object_layout_socket_metadata(path: &Path) -> std::io::Result<fs::Metadata> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_dir = open_object_layout_plain_directory(parent)?;
    let file_name = object_layout_plain_file_name(path)?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_PATH | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    fs::File::from(file_fd).metadata()
}

fn read_object_layout_control_file(path: &Path) -> std::io::Result<String> {
    read_object_layout_small_text_file(path, MAX_OBJECT_LAYOUT_CONTROL_BYTES)
}
