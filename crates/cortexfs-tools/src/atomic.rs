use crate::plain::{create_exclusive_file_at, open_plain_directory, plain_file_name};
use std::io;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn write_text_file_atomic(path: &Path, content: &str) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "path must have a parent directory",
        )
    })?;
    let name = plain_file_name(path)?;
    let directory = open_plain_directory(parent)?;
    for attempt in 0..16 {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let temporary = format!(".{name}.tmp-{}-{nonce}-{attempt}", std::process::id());
        match create_exclusive_file_at(&directory, &temporary, 0o644) {
            Ok(mut file) => {
                if let Err(error) = file
                    .write_all(content.as_bytes())
                    .and_then(|()| file.sync_all())
                {
                    remove_temporary(&directory, &temporary);
                    return Err(error);
                }
                drop(file);
                if let Err(error) =
                    nix::fcntl::renameat(&directory, temporary.as_str(), &directory, name)
                {
                    remove_temporary(&directory, &temporary);
                    return Err(io::Error::from(error));
                }
                return directory.sync_all();
            }
            Err(nix::errno::Errno::EEXIST) => {}
            Err(error) => return Err(io::Error::from(error)),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "cannot create unique temp file",
    ))
}

fn remove_temporary(directory: &std::fs::File, name: &str) {
    let _ignored = nix::unistd::unlinkat(directory, name, nix::unistd::UnlinkatFlags::NoRemoveDir);
}
