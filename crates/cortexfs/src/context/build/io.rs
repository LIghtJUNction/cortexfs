use super::*;
use crate::*;

use std::fs;
use std::io::Read;
use std::path::Path;

pub(crate) fn is_plain_dir_path(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_dir())
}

pub(crate) fn is_plain_file_path(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.is_file())
}

pub(crate) fn read_plain_text_file(path: &Path) -> std::io::Result<String> {
    let mut file = plain_fs::open_plain_file(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::other("path is not a plain file"));
    }
    if metadata.len() > MAX_CONTEXT_PACK_SOURCE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "context pack source is too large",
        ));
    }
    let len = usize::try_from(metadata.len())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.utf8_error()))
}

pub(crate) fn directory_entry_names(path: &Path) -> Result<Vec<String>, ContextPackBuildError> {
    let directory = plain_fs::open_plain_directory(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ContextPackBuildError::MissingSession
        } else {
            ContextPackBuildError::CannotRead
        }
    })?;
    let entries = fs::read_dir(plain_fs::proc_fd_path(&directory)).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ContextPackBuildError::MissingSession
        } else {
            ContextPackBuildError::CannotRead
        }
    })?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_error| ContextPackBuildError::CannotRead)?;
        let name = entry
            .file_name()
            .to_str()
            .ok_or(ContextPackBuildError::CannotRead)?
            .to_owned();
        names.push(name);
    }
    Ok(names)
}

pub(crate) fn is_safe_relative_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.bytes().any(|byte| byte.is_ascii_control())
        && name != "."
        && name != ".."
}

pub(crate) fn estimate_context_tokens(content: &str) -> u64 {
    let words = content.split_whitespace().count();
    u64::try_from(words.max(1)).unwrap_or(u64::MAX)
}
