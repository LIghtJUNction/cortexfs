use super::*;
use crate::*;
use std::fs::File;

pub(crate) fn push_str_byte_limit(output: &mut String, value: &str, max_bytes: usize) {
    if value.len() <= max_bytes {
        output.push_str(value);
        return;
    }
    let mut end = 0;
    for (index, character) in value.char_indices() {
        let next = index + character.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }
    if let Some(prefix) = value.get(..end) {
        output.push_str(prefix);
    }
}

pub(crate) fn read_bounded_regular_utf8(path: &Path, max_bytes: u64) -> Option<String> {
    let mut content = String::new();
    let file = support::plain::open_plain_file(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return None;
    }
    file.take(max_bytes.saturating_add(1))
        .read_to_string(&mut content)
        .ok()?;
    if u64::try_from(content.len()).ok()? > max_bytes {
        return None;
    }
    Some(content)
}

pub(crate) fn fd_entry_is_regular_file(parent_dir: &File, name: &str) -> bool {
    nix::sys::stat::fstatat(parent_dir, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
        .is_ok_and(|stat| stat.st_mode & libc::S_IFMT == libc::S_IFREG)
}

pub(crate) fn fd_entry_is_directory(parent_dir: &File, name: &str) -> bool {
    nix::sys::stat::fstatat(parent_dir, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
        .is_ok_and(|stat| stat.st_mode & libc::S_IFMT == libc::S_IFDIR)
}

pub(crate) fn read_history_messages_tail(session: &Path) -> std::io::Result<String> {
    columnar::tail_text(
        session,
        columnar::Stream::Messages,
        MAX_HISTORY_MESSAGES_READ_BYTES,
    )
}
