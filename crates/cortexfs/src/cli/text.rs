use crate::cli::nofollow::open_regular_file_no_follow;
use std::io::{self, Read};
use std::path::Path;

pub fn read_small_plain_text_file(
    path: &Path,
    max_bytes: u64,
    limit_label: &str,
) -> io::Result<String> {
    let mut file = open_regular_file_no_follow(path, nix::fcntl::OFlag::O_CLOEXEC)?;
    let metadata = file.metadata()?;
    if metadata.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file exceeds {limit_label} control read limit"),
        ));
    }
    let len = usize::try_from(metadata.len()).map_err(|error| {
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
