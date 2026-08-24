use std::os::unix::fs::MetadataExt;
use std::{fs, io::Write, os::unix::fs::PermissionsExt, path::Path};

use super::commit::commit_preserving;
use super::name::generated_sibling_name;
use super::{AtomicCommit, AtomicMetadata, AtomicReplaceOutcome, publish_outcome};
use crate::support::plain::{
    create_exclusive_file_at, open_plain_directory, plain_file_name, validate_plain_name,
};

pub(super) fn publish_path(
    path: &Path,
    content: &[u8],
    metadata: &AtomicMetadata,
    before_commit: Option<&mut dyn FnMut() -> std::io::Result<()>>,
) -> std::io::Result<AtomicReplaceOutcome> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_dir = open_plain_directory(parent)?;
    publish_at(
        &parent_dir,
        plain_file_name(path)?,
        content,
        metadata,
        before_commit,
    )
}

pub(super) fn publish_at(
    parent: &fs::File,
    name: &str,
    content: &[u8],
    metadata: &AtomicMetadata,
    mut before_commit: Option<&mut dyn FnMut() -> std::io::Result<()>>,
) -> std::io::Result<AtomicReplaceOutcome> {
    validate_plain_name(name)?;
    for attempt in 0..16 {
        let temp = generated_sibling_name(name, "tmp", attempt);
        let mut file = match create_exclusive_file_at(parent, &temp, metadata.mode) {
            Ok(file) => file,
            Err(nix::errno::Errno::EEXIST) => continue,
            Err(error) => return Err(error.into()),
        };
        let prepared = (|| {
            if let Some((uid, gid)) = metadata.owner {
                let created = file.metadata()?;
                if (created.uid(), created.gid()) != (uid, gid) {
                    nix::unistd::fchown(
                        &file,
                        Some(nix::unistd::Uid::from_raw(uid)),
                        Some(nix::unistd::Gid::from_raw(gid)),
                    )?;
                }
            }
            if metadata.exact_mode {
                file.set_permissions(fs::Permissions::from_mode(metadata.mode & 0o7777))?;
            }
            file.write_all(content)?;
            file.sync_all()?;
            if let Some(hook) = before_commit.as_deref_mut() {
                hook()?;
            }
            std::io::Result::Ok(())
        })();
        if let Err(error) = prepared {
            remove_temp(parent, &temp);
            return Err(error);
        }
        if let Some(identity) = metadata.identity {
            let replacement = match file.metadata() {
                Ok(value) => (value.dev(), value.ino()),
                Err(error) => {
                    remove_temp(parent, &temp);
                    return Err(error);
                }
            };
            drop(file);
            return commit_preserving(parent, &temp, name, identity, replacement);
        }
        drop(file);
        let renamed = match metadata.commit {
            AtomicCommit::Replace => nix::fcntl::renameat(parent, temp.as_str(), parent, name),
            AtomicCommit::NoReplace => nix::fcntl::renameat2(
                parent,
                temp.as_str(),
                parent,
                name,
                nix::fcntl::RenameFlags::RENAME_NOREPLACE,
            ),
        };
        if let Err(error) = renamed {
            remove_temp(parent, &temp);
            return Err(error.into());
        }
        return Ok(publish_outcome(parent.sync_all()));
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "cannot create unique temp file",
    ))
}

pub(super) fn remove_temp(parent: &fs::File, temp: &str) {
    let _ignored = nix::unistd::unlinkat(parent, temp, nix::unistd::UnlinkatFlags::NoRemoveDir);
}
