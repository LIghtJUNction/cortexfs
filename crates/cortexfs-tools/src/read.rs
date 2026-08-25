use crate::plain::{open_plain_directory, open_plain_file};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

pub(crate) fn read_small_text_file(path: &Path, max_bytes: u64) -> io::Result<String> {
    let file = open_plain_file(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "path is not a regular file",
        ));
    }
    if metadata.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds read limit",
        ));
    }
    read_file_to_string(file, metadata.len())
}

fn read_file_to_string(mut file: File, len: u64) -> io::Result<String> {
    let len = usize::try_from(len).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file is too large to read: {error}"),
        )
    })?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.utf8_error()))
}

pub(crate) fn create_plain_dir(path: &Path) -> io::Result<()> {
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        return if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            open_plain_directory(path)?.sync_all()
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path is not a plain directory",
            ))
        };
    }
    std::fs::create_dir_all(path)?;
    open_plain_directory(path)?.sync_all()
}
