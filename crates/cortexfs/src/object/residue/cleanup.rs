use crate::support::plain::{open_plain_directory, proc_fd_path};
use nix::libc;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::plan::{
    CleanupPlan, build_plan, open_plan_parent, open_relative_dir, validate_cleanup_path,
};
use super::{
    Receipt, ResidueCleanupReport, ResidueError, ResidueFileKind, conflict, receipt_at,
    require_receipt,
};

static CLEANUP_ID: AtomicU64 = AtomicU64::new(0);

struct DeleteContext<'a> {
    parent: &'a fs::File,
    original_name: &'a str,
    quarantine_name: &'a str,
    quarantine: &'a Path,
    requested: &'a Path,
    expected: Receipt,
}

impl DeleteContext<'_> {
    fn error(&self, stage: &'static str, detail: impl Into<String>) -> ResidueError {
        self.error_at(self.quarantine.to_path_buf(), stage, detail)
    }

    fn error_at(
        &self,
        quarantine: PathBuf,
        stage: &'static str,
        detail: impl Into<String>,
    ) -> ResidueError {
        conflict(
            self.requested,
            self.expected.dev,
            self.expected.ino,
            Some(quarantine),
            stage,
            detail,
        )
    }
}

pub(super) struct PreparedCleanup {
    parent: fs::File,
    parent_path: PathBuf,
    name: String,
    expected: Receipt,
    plan: CleanupPlan,
    report: ResidueCleanupReport,
}

struct ParkError {
    parked: Option<String>,
    detail: String,
}

impl ParkError {
    fn before(detail: impl Into<String>) -> Self {
        Self {
            parked: None,
            detail: detail.into(),
        }
    }

    fn retained(parked: String, detail: impl Into<String>) -> Self {
        Self {
            parked: Some(parked),
            detail: detail.into(),
        }
    }
}

/// Plans or applies cleanup for one exact install-stage receipt.
///
/// With `apply == false`, this performs the complete bounded preflight and does
/// not rename or remove anything. Rollback residue is never accepted. Callers
/// applying cleanup must quiesce processes with write authority to the source.
pub fn cleanup_residue(
    source: &Path,
    path: &Path,
    dev: u64,
    ino: u64,
    apply: bool,
) -> Result<ResidueCleanupReport, ResidueError> {
    let prepared = prepare_cleanup(source, path, dev, ino, apply)?;
    if apply {
        apply_cleanup(&prepared)?;
    }
    Ok(prepared.report)
}

pub(super) fn prepare_cleanup(
    source: &Path,
    path: &Path,
    dev: u64,
    ino: u64,
    apply: bool,
) -> Result<PreparedCleanup, ResidueError> {
    let (parent_path, name) = validate_cleanup_path(path)?;
    let source_dir = open_plain_directory(source).map_err(|error| {
        ResidueError::unavailable(format!("cannot open durable source: {error}"))
    })?;
    let source_metadata = source_dir.metadata().map_err(|error| {
        ResidueError::unavailable(format!("cannot inspect durable source: {error}"))
    })?;
    let parent = open_relative_dir(&source_dir, &parent_path, source_metadata.dev())?;
    let expected = Receipt {
        dev,
        ino,
        kind: ResidueFileKind::Directory,
    };
    let plan = build_plan(&parent, &name, expected, source_metadata.dev(), path)?;
    Ok(PreparedCleanup {
        parent,
        parent_path,
        name,
        expected,
        report: ResidueCleanupReport {
            path: path.to_path_buf(),
            dev,
            ino,
            entries: plan.entries.len(),
            applied: apply,
        },
        plan,
    })
}

pub(super) fn apply_cleanup(prepared: &PreparedCleanup) -> Result<(), ResidueError> {
    let path = &prepared.report.path;
    let dev = prepared.report.dev;
    let ino = prepared.report.ino;
    let quarantine_name = park_exact(
        &prepared.parent,
        OsStr::new(&prepared.name),
        prepared.expected,
        ".cortexfs-cleanup",
    )
    .map_err(|error| {
        let quarantine = error.parked.map(|parked| prepared.parent_path.join(parked));
        conflict(path, dev, ino, quarantine, "top-isolate", error.detail)
    })?;
    let quarantine = prepared.parent_path.join(&quarantine_name);
    let context = DeleteContext {
        parent: &prepared.parent,
        original_name: &prepared.name,
        quarantine_name: &quarantine_name,
        quarantine: &quarantine,
        requested: path,
        expected: prepared.expected,
    };
    let result = (|| {
        require_missing(&prepared.parent, OsStr::new(&prepared.name))
            .map_err(|detail| context.error("original-recreated", detail))?;
        prepared
            .parent
            .sync_all()
            .map_err(|error| context.error("top-sync", error.to_string()))?;
        delete_plan(&context, &prepared.plan)
    })();
    result.map_err(|error| restore_stage(&context, error))
}

fn delete_plan(context: &DeleteContext<'_>, plan: &CleanupPlan) -> Result<(), ResidueError> {
    delete_planned_entries(context, plan)?;
    require_empty(&plan.top).map_err(|detail| context.error("nonempty-addition", detail))?;
    plan.top
        .sync_all()
        .map_err(|error| context.error("tree-sync", error.to_string()))?;
    delete_top(context)
}

fn restore_stage(context: &DeleteContext<'_>, error: ResidueError) -> ResidueError {
    let ResidueError::Conflict(mut issue) = error else {
        return error;
    };
    let nested = issue
        .quarantine
        .as_ref()
        .and_then(|path| path.strip_prefix(context.quarantine).ok())
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf);
    let quarantine_name = OsStr::new(context.quarantine_name);
    let actual = match receipt_at(context.parent, quarantine_name) {
        Ok(actual) => actual,
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => {
            issue.quarantine = None;
            return ResidueError::Conflict(issue);
        }
        Err(error) => {
            issue.quarantine = Some(context.quarantine.to_path_buf());
            issue.detail = format!(
                "{}; cannot inspect top quarantine for restoration: {error}",
                issue.detail
            );
            return ResidueError::Conflict(issue);
        }
    };
    if actual != context.expected {
        issue.quarantine = Some(context.quarantine.to_path_buf());
        issue.detail = format!(
            "{}; top quarantine receipt changed and was not restored",
            issue.detail
        );
        return ResidueError::Conflict(issue);
    }
    if let Err(error) = nix::fcntl::renameat2(
        context.parent,
        context.quarantine_name,
        context.parent,
        context.original_name,
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    ) {
        issue.detail = format!("{}; cannot restore install stage: {error}", issue.detail);
        issue.quarantine = match receipt_at(context.parent, quarantine_name) {
            Ok(_) => Some(context.quarantine.to_path_buf()),
            Err(error) if error.raw_os_error() == Some(libc::ENOENT) => None,
            Err(inspect_error) => {
                issue.detail = format!(
                    "{}; cannot inspect top quarantine after restore failure: {inspect_error}",
                    issue.detail
                );
                Some(context.quarantine.to_path_buf())
            }
        };
        return ResidueError::Conflict(issue);
    }

    let restored = require_receipt(
        context.parent,
        OsStr::new(context.original_name),
        context.expected,
    )
    .and_then(|()| require_missing(context.parent, quarantine_name))
    .and_then(|()| {
        context
            .parent
            .sync_all()
            .map_err(|error| format!("cannot sync restored install stage: {error}"))
    });
    match restored {
        Ok(()) => {
            issue.quarantine = nested.map(|path| context.requested.join(path));
            issue.detail = format!("{}; restored original install-stage name", issue.detail);
        }
        Err(error) => {
            issue.detail = format!(
                "{}; restored install-stage rename could not be verified: {error}",
                issue.detail
            );
            issue.quarantine = match receipt_at(context.parent, quarantine_name) {
                Ok(_) => Some(context.quarantine.to_path_buf()),
                Err(receipt_error) if receipt_error.raw_os_error() == Some(libc::ENOENT) => None,
                Err(inspect_error) => {
                    issue.detail = format!(
                        "{}; cannot inspect top quarantine after verification failure: {inspect_error}",
                        issue.detail
                    );
                    Some(context.quarantine.to_path_buf())
                }
            };
        }
    }
    ResidueError::Conflict(issue)
}

fn delete_planned_entries(
    context: &DeleteContext<'_>,
    plan: &CleanupPlan,
) -> Result<(), ResidueError> {
    let mut paths: Vec<PathBuf> = plan
        .entries
        .keys()
        .filter(|path| !path.as_os_str().is_empty())
        .cloned()
        .collect();
    paths.sort_by(|left, right| {
        right
            .components()
            .count()
            .cmp(&left.components().count())
            .then_with(|| left.cmp(right))
    });
    for path in paths {
        let receipt = plan
            .entries
            .get(&path)
            .copied()
            .ok_or_else(|| context.error("plan", "missing cleanup receipt"))?;
        let entry_parent = open_plan_parent(&plan.top, &path, &plan.entries)
            .map_err(|detail| context.error("parent-recheck", detail))?;
        let name = path
            .file_name()
            .ok_or_else(|| context.error("plan", "cleanup entry has no name"))?;
        delete_entry(&entry_parent, name, receipt).map_err(|error| {
            let quarantine = error.parked.map_or_else(
                || context.quarantine.to_path_buf(),
                |parked| {
                    context
                        .quarantine
                        .join(path.parent().unwrap_or_else(|| Path::new("")))
                        .join(parked)
                },
            );
            context.error_at(
                quarantine,
                "entry-delete",
                format!("{}: {}", path.display(), error.detail),
            )
        })?;
    }
    Ok(())
}

fn delete_top(context: &DeleteContext<'_>) -> Result<(), ResidueError> {
    require_receipt(
        context.parent,
        OsStr::new(context.quarantine_name),
        context.expected,
    )
    .map_err(|detail| context.error("quarantine-recheck", detail))?;
    require_missing(context.parent, OsStr::new(context.original_name))
        .map_err(|detail| context.error("original-recreated", detail))?;
    nix::unistd::unlinkat(
        context.parent,
        context.quarantine_name,
        nix::unistd::UnlinkatFlags::RemoveDir,
    )
    .map_err(|error| context.error("top-delete", error.to_string()))?;
    require_missing(context.parent, OsStr::new(context.quarantine_name))
        .map_err(|detail| context.error("top-delete-postcheck", detail))?;
    require_missing(context.parent, OsStr::new(context.original_name))
        .map_err(|detail| context.error("original-recreated", detail))?;
    context
        .parent
        .sync_all()
        .map_err(|error| context.error("final-sync", error.to_string()))
}

fn require_empty(directory: &fs::File) -> Result<(), String> {
    let mut entries = fs::read_dir(proc_fd_path(directory))
        .map_err(|error| format!("cannot enumerate isolated install stage: {error}"))?;
    match entries.next() {
        None => Ok(()),
        Some(Ok(_entry)) => Err("isolated install stage gained an unplanned entry".to_owned()),
        Some(Err(error)) => Err(format!("cannot enumerate isolated install stage: {error}")),
    }
}

fn delete_entry(parent: &fs::File, name: &OsStr, receipt: Receipt) -> Result<(), ParkError> {
    let parked = park_exact(parent, name, receipt, ".cortexfs-cleanup-entry")?;
    require_missing(parent, name).map_err(|detail| ParkError::retained(parked.clone(), detail))?;
    parent.sync_all().map_err(|error| {
        ParkError::retained(
            parked.clone(),
            format!("cannot sync parked cleanup entry: {error}"),
        )
    })?;
    if let Err(detail) = require_receipt(parent, OsStr::new(&parked), receipt) {
        let moved = receipt_at(parent, OsStr::new(&parked)).ok();
        return Err(restore_parked(parent, name, parked, moved, detail));
    }
    let flags = if receipt.kind == ResidueFileKind::Directory {
        nix::unistd::UnlinkatFlags::RemoveDir
    } else {
        nix::unistd::UnlinkatFlags::NoRemoveDir
    };
    nix::unistd::unlinkat(parent, parked.as_str(), flags).map_err(|error| {
        ParkError::retained(
            parked.clone(),
            format!("cannot unlink parked cleanup entry: {error}"),
        )
    })?;
    require_missing(parent, OsStr::new(&parked))
        .map_err(|detail| ParkError::retained(parked, detail))?;
    require_missing(parent, name).map_err(ParkError::before)?;
    parent
        .sync_all()
        .map_err(|error| ParkError::before(format!("cannot sync deleted cleanup entry: {error}")))
}

fn park_exact(
    parent: &fs::File,
    name: &OsStr,
    receipt: Receipt,
    prefix: &str,
) -> Result<String, ParkError> {
    let parked = rename_unique(parent, name, prefix).map_err(ParkError::before)?;
    if let Err(detail) = require_receipt(parent, OsStr::new(&parked), receipt) {
        let moved = receipt_at(parent, OsStr::new(&parked)).ok();
        return Err(restore_parked(parent, name, parked, moved, detail));
    }
    Ok(parked)
}

fn restore_parked(
    parent: &fs::File,
    original: &OsStr,
    parked: String,
    moved: Option<Receipt>,
    detail: impl Into<String>,
) -> ParkError {
    let detail = detail.into();
    if let Err(error) = nix::fcntl::renameat2(
        parent,
        parked.as_str(),
        parent,
        original,
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    ) {
        return match require_missing(parent, OsStr::new(&parked)) {
            Ok(()) => ParkError::before(format!(
                "{detail}; cannot restore isolated entry: {error}; parked entry is missing"
            )),
            Err(status_detail) => ParkError::retained(
                parked,
                format!(
                    "{detail}; cannot restore isolated entry: {error}; parked entry status: {status_detail}"
                ),
            ),
        };
    }

    if let Err(error) = require_missing(parent, OsStr::new(&parked)) {
        return ParkError::retained(
            parked,
            format!("{detail}; restored original name but parked path remains: {error}"),
        );
    }
    let verify = moved
        .map_or(Ok(()), |receipt| require_receipt(parent, original, receipt))
        .and_then(|()| {
            parent
                .sync_all()
                .map_err(|error| format!("cannot sync restored cleanup entry: {error}"))
        });
    match verify {
        Ok(()) => ParkError::before(format!("{detail}; restored original name")),
        Err(error) => ParkError::before(format!(
            "{detail}; restored original name but verification failed: {error}"
        )),
    }
}

fn rename_unique(parent: &fs::File, name: &OsStr, prefix: &str) -> Result<String, String> {
    for _attempt in 0..32 {
        let id = CLEANUP_ID.fetch_add(1, Ordering::Relaxed);
        let parked = format!("{prefix}-{}-{id}", std::process::id());
        match nix::fcntl::renameat2(
            parent,
            name,
            parent,
            parked.as_str(),
            nix::fcntl::RenameFlags::RENAME_NOREPLACE,
        ) {
            Ok(()) => return Ok(parked),
            Err(nix::errno::Errno::EEXIST) => {}
            Err(error) => return Err(format!("cannot isolate cleanup entry: {error}")),
        }
    }
    Err("cannot allocate a unique cleanup quarantine".to_owned())
}

fn require_missing(parent: &fs::File, name: &OsStr) -> Result<(), String> {
    match receipt_at(parent, name) {
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => Ok(()),
        Ok(receipt) => Err(format!(
            "entry still exists with dev={} ino={} type={}",
            receipt.dev,
            receipt.ino,
            receipt.kind.as_str()
        )),
        Err(error) => Err(format!("cannot verify entry absence: {error}")),
    }
}
