use crate::*;

pub(crate) fn open_log(path: &Path) -> Result<std::fs::File, CtxtermError> {
    if let Some(parent) = path.parent() {
        create_plain_directory(
            parent,
            0o700,
            "ctxterm parent path is not a plain directory",
            "ctxterm path contains a non-directory entry",
            "invalid ctxterm directory name",
        )
        .map_err(|error| {
            CtxtermError::unavailable(format!("cannot create {}: {error}", parent.display()))
        })?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK | nix::libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| {
            CtxtermError::unavailable(format!("cannot open {}: {error}", path.display()))
        })?;
    if !file
        .metadata()
        .map_err(|error| {
            CtxtermError::unavailable(format!("cannot inspect {}: {error}", path.display()))
        })?
        .is_file()
    {
        return Err(CtxtermError::unavailable(format!(
            "{} is not a plain file",
            path.display()
        )));
    }
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|error| {
            CtxtermError::unavailable(format!("cannot chmod {}: {error}", path.display()))
        })?;
    Ok(file)
}
