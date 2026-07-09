use crate::*;
use cortexfs::plain_fs::{CreatePlainDirMessages, create_plain_dir_with};

pub(crate) fn create_plain_directory(
    path: &Path,
    mode: u32,
    existing_not_dir_message: &'static str,
    contains_non_dir_message: &'static str,
    invalid_name_message: &'static str,
) -> io::Result<()> {
    create_plain_dir_with(
        path,
        CreatePlainDirMessages {
            mode,
            existing_not_dir_kind: io::ErrorKind::AlreadyExists,
            existing_not_dir_message,
            contains_non_dir_kind: io::ErrorKind::AlreadyExists,
            contains_non_dir_message,
            invalid_name_message,
        },
    )
}
