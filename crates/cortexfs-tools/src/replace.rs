use crate::atomic::write_text_file_atomic;
use crate::read::read_small_text_file;
use cortexfs_tool_sdk::ToolError;
use std::io;
use std::path::Path;

pub fn replace_exactly_once(path: &Path, old: &str, new: &str) -> io::Result<()> {
    if old.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "old text must not be empty",
        ));
    }
    let content = read_small_text_file(path, crate::MAX_FS_READ_BYTES)?;
    let first = content.find(old);
    if first.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "old text not found",
        ));
    }
    if content.rfind(old) != first {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "old text occurs more than once",
        ));
    }
    write_text_file_atomic(path, &content.replacen(old, new, 1))
}

#[must_use]
pub fn fs_replace_tool_error(error: &io::Error) -> ToolError {
    match error.kind() {
        io::ErrorKind::NotFound => ToolError::not_found(error.to_string()),
        io::ErrorKind::InvalidInput => ToolError::invalid(error.to_string()),
        _ => ToolError::denied("replace failed"),
    }
}
