use super::install::InstallError;
#[cfg(test)]
use crate::support::plain::proc_fd_path;
use crate::support::receipt::{EntryKind, EntryReceipt, entry_matches, park_entry, restore_entry};

#[cfg(test)]
use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
thread_local! {
    static RECREATED_SOURCE: Cell<Option<&'static str>> = const { Cell::new(None) };
}

#[derive(Clone, Copy, Default, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum PairStep {
    #[default]
    Start,
    ExecutableSynced,
    ExecutableMissing,
    ControlSynced,
    Complete,
}

#[derive(Default)]
pub(super) struct PairProgress {
    pub(super) phase: PairStep,
    pub(super) completed: [bool; 2],
    pub(super) observed: [bool; 2],
    pub(super) detail: Option<String>,
}

pub(super) fn quarantine_pair(
    dirs: [&fs::File; 2],
    source: [&str; 2],
    target: [&str; 2],
    receipt: [EntryReceipt; 2],
    mut progress: PairProgress,
) -> PairProgress {
    let [class, stage] = dirs;
    let [source_executable, source_control] = source;
    let [target_executable, target_control] = target;
    let [executable, control] = receipt;
    let [completed_executable, completed_control] = progress.completed;
    let [observed_executable, observed_control] = progress.observed;
    macro_rules! attempt {
        ($operation:expr) => {
            if let Err(detail) = $operation {
                progress.detail = Some(detail);
                return progress;
            }
        };
    }
    match progress.phase {
        PairStep::Start | PairStep::ExecutableMissing => {
            let (source_name, target_name, entry, kind, synced) = match progress.phase {
                PairStep::Start => {
                    attempt!(require_entry(
                        class,
                        source_executable,
                        executable,
                        EntryKind::Executable,
                    ));
                    attempt!(require_entry(
                        class,
                        source_control,
                        control,
                        EntryKind::Directory,
                    ));
                    (
                        source_executable,
                        target_executable,
                        executable,
                        EntryKind::Executable,
                        PairStep::ExecutableSynced,
                    )
                }
                _ => (
                    source_control,
                    target_control,
                    control,
                    EntryKind::Directory,
                    PairStep::ControlSynced,
                ),
            };
            let result = move_exact(class, source_name, stage, target_name, entry, kind);
            let observed = entry_matches(stage, target_name, entry, kind);
            progress.observed = match progress.phase {
                PairStep::Start => [observed, observed_control],
                _ => [observed_executable, observed],
            };
            attempt!(result);
            progress.completed = match progress.phase {
                PairStep::Start => [true, completed_control],
                _ => [completed_executable, true],
            };
            attempt!(sync_dirs(class, stage));
            progress.phase = synced;
        }
        PairStep::ExecutableSynced => {
            attempt!(require_missing(class, source_executable));
            progress.phase = PairStep::ExecutableMissing;
        }
        PairStep::ControlSynced => {
            attempt!(require_missing(class, source_executable));
            attempt!(require_missing(class, source_control));
            progress.phase = PairStep::Complete;
        }
        PairStep::Complete => {}
    }
    progress
}

pub(super) fn relative_stage(
    root: &Path,
    class: &Path,
    stage: &str,
) -> Result<PathBuf, InstallError> {
    class
        .join(stage)
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .map_err(|_error| InstallError::unavailable("cannot derive object residue path"))
}

pub(super) fn move_exact(
    source: &fs::File,
    source_name: &str,
    target: &fs::File,
    target_name: &str,
    receipt: EntryReceipt,
    kind: EntryKind,
) -> Result<(), String> {
    park_entry(source, source_name, target, target_name, receipt, kind)
        .map_err(|error| format!("cannot quarantine object entry: {error}"))
}

#[cfg(test)]
pub(super) fn set_recreated_source(name: Option<&'static str>) -> Option<&'static str> {
    let previous = RECREATED_SOURCE.with(|fault| fault.replace(name));
    let hook = name.map(|_| -> crate::support::receipt::ParkHook { Box::new(recreate_hook) });
    crate::support::receipt::set_park_hook(hook);
    previous
}

#[cfg(test)]
fn recreate_hook(source: &fs::File, source_name: &str) -> std::io::Result<()> {
    if RECREATED_SOURCE.with(Cell::get) != Some(source_name) {
        let _previous = crate::support::receipt::set_park_hook(Some(Box::new(recreate_hook)));
        return Ok(());
    }
    recreate_source(source, source_name).map_err(std::io::Error::other)
}

#[cfg(test)]
fn recreate_source(source: &fs::File, source_name: &str) -> Result<(), String> {
    let enabled = RECREATED_SOURCE.with(|fault| {
        if fault.get() == Some(source_name) {
            fault.set(None);
            true
        } else {
            false
        }
    });
    if !enabled {
        return Ok(());
    }
    std::os::unix::net::UnixListener::bind(proc_fd_path(source).join(source_name))
        .map(|_listener| ())
        .map_err(|error| format!("cannot recreate moved stage source: {error}"))
}

pub(super) fn restore_exact(
    source: &fs::File,
    source_name: &str,
    target: &fs::File,
    target_name: &str,
    receipt: EntryReceipt,
    kind: EntryKind,
) -> Result<(), String> {
    require_entry(source, source_name, receipt, kind)?;
    require_missing(target, target_name)?;
    restore_entry(source, source_name, target, target_name, receipt, kind)
        .map_err(|error| format!("cannot restore quarantined object entry: {error}"))
}

pub(super) fn require_entry(
    parent: &fs::File,
    name: &str,
    receipt: EntryReceipt,
    kind: EntryKind,
) -> Result<(), String> {
    if entry_matches(parent, name, receipt, kind) {
        Ok(())
    } else {
        Err(format!(
            "object entry receipt changed: {name} expected dev={} ino={}",
            receipt.dev, receipt.ino
        ))
    }
}

pub(super) fn require_missing(parent: &fs::File, name: &str) -> Result<(), String> {
    match nix::sys::stat::fstatat(parent, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW) {
        Err(nix::errno::Errno::ENOENT) => Ok(()),
        Ok(stat) => Err(format!(
            "object path was recreated: {name} dev={} ino={}",
            stat.st_dev, stat.st_ino
        )),
        Err(error) => Err(format!(
            "cannot verify object path absence: {name}: {error}"
        )),
    }
}

pub(super) fn sync_dirs(first: &fs::File, second: &fs::File) -> Result<(), String> {
    first
        .sync_all()
        .map_err(|error| format!("cannot sync object class: {error}"))?;
    second
        .sync_all()
        .map_err(|error| format!("cannot sync object quarantine: {error}"))
}

#[cfg(test)]
pub(super) fn create_foreign_executable(parent: &fs::File, name: &str) -> Result<(), String> {
    use std::io::Write as _;

    let fd = nix::fcntl::openat(
        parent,
        name,
        nix::fcntl::OFlag::O_WRONLY
            | nix::fcntl::OFlag::O_CREAT
            | nix::fcntl::OFlag::O_EXCL
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::from_bits_truncate(0o755),
    )
    .map_err(|error| format!("cannot create foreign executable: {error}"))?;
    let mut file = fs::File::from(fd);
    file.write_all(b"foreign")
        .map_err(|error| format!("cannot write foreign executable: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("cannot sync foreign executable: {error}"))?;
    parent
        .sync_all()
        .map_err(|error| format!("cannot sync foreign executable parent: {error}"))
}

#[cfg(test)]
pub(super) fn create_foreign_control(parent: &fs::File, name: &str) -> Result<(), String> {
    nix::sys::stat::mkdirat(
        parent,
        name,
        nix::sys::stat::Mode::from_bits_truncate(0o700),
    )
    .map_err(|error| format!("cannot create foreign control: {error}"))?;
    parent
        .sync_all()
        .map_err(|error| format!("cannot sync foreign control: {error}"))
}
