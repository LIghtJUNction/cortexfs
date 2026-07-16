use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use nix::fcntl::{Flock, FlockArg};

use super::bootstrap::{
    BootstrapAction, REFERENCE_AGENTS, bootstrap_state_matches_target, ensure_v1_reference_tree,
    plan_reference_tree_upgrade, read_bootstrap_state,
};

/// Default host storage directory containing versioned `CortexFS` generations.
pub const SYSTEM_STORAGE_DIR: &str = "/var/lib/cortexfs/storage";
/// Stable host path selected by the system mount and agent runtime.
pub const SYSTEM_STORAGE_CURRENT: &str = "/var/lib/cortexfs/storage/current";

/// Error while staging or atomically selecting a storage generation.
#[derive(Debug, thiserror::Error)]
pub enum StorageUpdateError {
    /// Storage layout or staged reference-tree validation failed.
    #[error("{0}")]
    Invalid(&'static str),
    /// A filesystem or clone operation failed.
    #[error("{0}")]
    Io(std::io::Error),
}

impl From<std::io::Error> for StorageUpdateError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Stages, validates, and atomically selects the next reference-tree generation.
pub fn update_storage_generation(storage: &Path) -> Result<PathBuf, StorageUpdateError> {
    update_storage_generation_with_prune(storage, false)
}

/// Stages a generation and optionally removes every non-current plain generation.
pub fn update_storage_generation_with_prune(
    storage: &Path,
    prune: bool,
) -> Result<PathBuf, StorageUpdateError> {
    require_plain_or_create(storage)?;
    let generations = storage.join("generations");
    require_plain_or_create(&generations)?;
    let lock = open_lock(&storage.join(".update.lock"))?;
    let _lock = Flock::lock(lock, FlockArg::LockExclusiveNonblock).map_err(|(_file, _error)| {
        StorageUpdateError::Invalid("storage update is already running")
    })?;

    let current = current_generation(storage, &generations)?;
    if let Some(current) = current.as_deref()
        && validate_generation(current).is_ok()
    {
        if prune {
            prune_generations(&generations, current)?;
        }
        return Ok(current.to_path_buf());
    }
    let name = generation_name();
    let stage = generations.join(format!(".stage-{name}"));
    if fs::symlink_metadata(&stage).is_ok() {
        return Err(StorageUpdateError::Invalid("staging generation exists"));
    }
    fs::create_dir_all(&stage)?;
    let result = stage_generation(storage, &generations, current.as_deref(), &stage, &name);
    if result.is_err() {
        let _ignored = fs::remove_dir_all(&stage);
    }
    let generation = result?;
    if prune {
        prune_generations(&generations, &generation)?;
    }
    Ok(generation)
}

fn prune_generations(generations: &Path, current: &Path) -> Result<(), StorageUpdateError> {
    let current_name = current.file_name().ok_or(StorageUpdateError::Invalid(
        "current generation name is invalid",
    ))?;
    for entry in fs::read_dir(generations)? {
        let entry = entry?;
        if entry.file_name() == current_name {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            fs::remove_dir_all(entry.path())?;
        }
    }
    sync_dir(generations)
}

fn stage_generation(
    storage: &Path,
    generations: &Path,
    current: Option<&Path>,
    stage: &Path,
    name: &str,
) -> Result<PathBuf, StorageUpdateError> {
    if let Some(source) = current {
        let status = Command::new("/usr/bin/cp")
            .args(["--archive", "--reflink=auto", "--"])
            .arg(source.join("."))
            .arg(stage)
            .status()?;
        if !status.success() {
            return Err(StorageUpdateError::Invalid(
                "cannot clone current generation",
            ));
        }
    }
    ensure_v1_reference_tree(stage)
        .map_err(|_error| StorageUpdateError::Invalid("cannot bootstrap staged generation"))?;
    validate_generation(stage)?;

    let generation = generations.join(name);
    fs::rename(stage, &generation)?;
    sync_dir(generations)?;
    switch_current(storage, Path::new("generations").join(name))?;
    Ok(generation)
}

fn current_generation(
    storage: &Path,
    generations: &Path,
) -> Result<Option<PathBuf>, StorageUpdateError> {
    let current = storage.join("current");
    let target = match fs::read_link(&current) {
        Ok(target) => target,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut components = target.components();
    let first = components.next();
    let second = components.next();
    if first.is_none_or(|component| component.as_os_str() != "generations")
        || second.is_none_or(|component| {
            !safe_generation_name(component.as_os_str().to_string_lossy().as_ref())
        })
        || components.next().is_some()
    {
        return Err(StorageUpdateError::Invalid(
            "current generation symlink is invalid",
        ));
    }
    let path = storage.join(&target);
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || path.parent() != Some(generations)
    {
        return Err(StorageUpdateError::Invalid(
            "current generation is not a plain directory",
        ));
    }
    Ok(Some(path))
}

fn switch_current(storage: &Path, target: PathBuf) -> Result<(), StorageUpdateError> {
    let temporary = storage.join(format!(".current-{}", std::process::id()));
    if fs::symlink_metadata(&temporary).is_ok() {
        return Err(StorageUpdateError::Invalid("temporary current link exists"));
    }
    symlink(target, &temporary)?;
    sync_dir(storage)?;
    fs::rename(&temporary, storage.join("current"))?;
    sync_dir(storage)
}

fn validate_generation(root: &Path) -> Result<(), StorageUpdateError> {
    if !read_bootstrap_state(root).is_some_and(|state| bootstrap_state_matches_target(&state)) {
        return Err(StorageUpdateError::Invalid(
            "staged bootstrap state is invalid",
        ));
    }
    if plan_reference_tree_upgrade(root)
        .actions
        .iter()
        .any(|action| {
            matches!(
                action,
                BootstrapAction::EnsureAgent { .. } | BootstrapAction::WriteState { .. }
            )
        })
    {
        return Err(StorageUpdateError::Invalid(
            "staged generation is not fully upgraded",
        ));
    }
    for agent in REFERENCE_AGENTS {
        let control = root.join("agent").join(format!("{}.d", agent.name));
        let model = fs::read_to_string(control.join("model"))?;
        let model = model.trim();
        let policy = fs::read_to_string(control.join("policy"))?;
        let expected = format!("allow {}_t model:{model} use", agent.name);
        let grants = policy
            .lines()
            .filter(|line| line.contains(" model:") && line.ends_with(" use"))
            .collect::<Vec<_>>();
        if grants != [expected.as_str()] {
            return Err(StorageUpdateError::Invalid(
                "agent model policy is incoherent",
            ));
        }
    }
    Ok(())
}

fn generation_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!(
        "v{}-{nanos}-{}",
        super::bootstrap::REFERENCE_TREE_VERSION,
        std::process::id()
    )
}

fn safe_generation_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'))
}

fn require_plain_or_create(path: &Path) -> Result<(), StorageUpdateError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(StorageUpdateError::Invalid(
            "storage path is not a plain directory",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn open_lock(path: &Path) -> Result<File, StorageUpdateError> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(path)
        .map_err(StorageUpdateError::Io)?;
    if !file.metadata()?.is_file() {
        return Err(StorageUpdateError::Invalid(
            "storage lock is not a plain file",
        ));
    }
    Ok(file)
}

/// Resolves a mutable storage selector once for a long-lived process.
pub fn pin_storage_source(source: &Path) -> Result<PathBuf, StorageUpdateError> {
    let pinned = fs::canonicalize(source)?;
    let metadata = fs::symlink_metadata(&pinned)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(StorageUpdateError::Invalid(
            "storage source is not a plain directory",
        ));
    }
    Ok(pinned)
}

fn sync_dir(path: &Path) -> Result<(), StorageUpdateError> {
    File::open(path)?.sync_all().map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    #[test]
    fn fresh_storage_stages_canonical_generation() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let storage = directory.path().join("storage");
        let generation = update_storage_generation_with_prune(&storage, true)?;

        assert_eq!(
            fs::read_link(storage.join("current"))?,
            generation.strip_prefix(&storage)?
        );
        validate_generation(&generation)?;
        assert_eq!(update_storage_generation(&storage)?, generation);
        assert_eq!(fs::read_dir(storage.join("generations"))?.count(), 1);
        Ok(())
    }

    #[test]
    fn pinned_projection_ignores_later_current_switch() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let storage = directory.path().join("storage");
        let generations = storage.join("generations");
        let old = generations.join("old");
        let new = generations.join("new");
        fs::create_dir_all(&old)?;
        fs::create_dir_all(&new)?;
        fs::write(old.join("status"), "old\n")?;
        fs::write(new.join("status"), "new\n")?;
        symlink("generations/old", storage.join("current"))?;
        let pinned = pin_storage_source(&storage.join("current"))?;
        let projection = crate::FuseV1Projection::new(&pinned);

        symlink("generations/new", storage.join(".next"))?;
        fs::rename(storage.join(".next"), storage.join("current"))?;

        assert_eq!(projection.root(), old);
        assert_eq!(projection.read_to_string("status"), Ok("old\n".to_owned()));
        assert_eq!(fs::read_to_string(storage.join("current/status"))?, "new\n");
        Ok(())
    }
}
