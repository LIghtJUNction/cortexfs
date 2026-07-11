use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::support::layout_path::require_symlink_dir;
use crate::{PathLayoutIssue, SHARED_QUEUE_REQUIRED_DIRS, is_object_name};

/// Shared queue layout uses the shared path-layout issue model.
pub type SharedQueueLayoutIssue = PathLayoutIssue;

/// Result of inspecting a shared queue directory.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SharedQueueLayoutReport {
    issues: Vec<PathLayoutIssue>,
}

impl_issue_report!(SharedQueueLayoutReport, PathLayoutIssue);

/// Error while claiming a shared queue job.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedQueueClaimError {
    /// Worker name is not a valid object-like token.
    InvalidWorkerName,
    /// Queue `pending/` cannot be read.
    CannotReadPending,
    /// Queue layout contains a symlink or non-directory where a queue directory is required.
    InvalidQueueDirectory,
    /// A pending job entry could not be inspected.
    CannotInspectJob,
    /// Another worker already claimed the job or claim directory cannot be created.
    CannotCreateClaim,
    /// Pending job could not be moved into its claim directory.
    CannotClaimJob,
    /// The claimed job could not be recorded in `lease/`.
    CannotRecordLease,
}

/// Error while finishing a shared queue job.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedQueueFinishError {
    /// Job name is not a valid `*.req.json` queue request.
    InvalidJobName,
    /// Result could not be written into `done/` or `failed/`.
    CannotWriteResult,
    /// Queue layout contains a symlink or non-directory where a queue directory is required.
    InvalidQueueDirectory,
    /// Claimed job file could not be moved into `done/` or `failed/`.
    CannotMoveClaimedJob,
    /// Claim or lease cleanup failed.
    CannotCleanup,
}

/// Error while recovering a shared queue job claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedQueueRecoverError {
    /// Job name is not a valid `*.req.json` queue request.
    InvalidJobName,
    /// No claimed job file exists to recover.
    MissingClaim,
    /// No lease exists for the claimed job.
    MissingLease,
    /// Queue layout contains a symlink or non-directory where a queue directory is required.
    InvalidQueueDirectory,
    /// Claimed job could not be moved back into `pending/`.
    CannotRequeue,
    /// Claim or lease cleanup failed.
    CannotCleanup,
}

/// Terminal shared queue outcome directory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedQueueOutcome {
    /// Successful job result under `done/`.
    Done,
    /// Failed job result under `failed/`.
    Failed,
}

impl SharedQueueOutcome {
    /// Returns the stable queue directory for this outcome.
    #[must_use]
    pub const fn as_dir(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }
}

/// Claimed shared queue job.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedQueueClaim {
    job_name: String,
    claimed_path: PathBuf,
    lease_path: PathBuf,
}

impl SharedQueueClaim {
    /// Creates a claimed job record.
    #[must_use]
    pub fn new(job_name: String, claimed_path: PathBuf, lease_path: PathBuf) -> Self {
        Self {
            job_name,
            claimed_path,
            lease_path,
        }
    }

    /// Returns the claimed job file name.
    #[must_use]
    pub fn job_name(&self) -> &str {
        &self.job_name
    }

    /// Returns the claimed job path.
    #[must_use]
    pub fn claimed_path(&self) -> &Path {
        &self.claimed_path
    }

    /// Returns the recoverable lease directory path.
    #[must_use]
    pub fn lease_path(&self) -> &Path {
        &self.lease_path
    }
}

/// Inspects a shared project queue for the v1 recommended directory shape.
#[must_use]
pub fn inspect_shared_queue_layout(queue_dir: &Path) -> SharedQueueLayoutReport {
    let mut issues = Vec::new();
    for dir in SHARED_QUEUE_REQUIRED_DIRS {
        require_symlink_dir(&queue_dir.join(dir), dir, &mut issues);
    }
    SharedQueueLayoutReport::new(issues)
}

/// Claims the first pending shared queue job using a mkdir lock plus atomic
/// rename into `claimed/`.
///
/// Pending entries are ordinary files under `pending/`. Invalid names and
/// non-files are ignored. The claimed job is moved to
/// `claimed/<job-name>/<job-name>`, and `lease/<job-name>/worker` records the
/// worker that claimed it. If another worker wins a race, this function skips
/// that job and tries the next pending entry.
pub fn claim_next_shared_queue_job(
    queue_dir: &Path,
    worker_name: &str,
) -> Result<Option<SharedQueueClaim>, SharedQueueClaimError> {
    if !is_object_name(worker_name) {
        return Err(SharedQueueClaimError::InvalidWorkerName);
    }

    let pending_dir = queue_child_dir(queue_dir, "pending")
        .map_err(|_error| SharedQueueClaimError::InvalidQueueDirectory)?;
    let pending_dir_fd = queue_child_dir_fd(queue_dir, "pending")
        .map_err(|_error| SharedQueueClaimError::InvalidQueueDirectory)?;
    let claimed_root = queue_child_dir(queue_dir, "claimed")
        .map_err(|_error| SharedQueueClaimError::InvalidQueueDirectory)?;
    let claimed_root_fd = queue_child_dir_fd(queue_dir, "claimed")
        .map_err(|_error| SharedQueueClaimError::InvalidQueueDirectory)?;
    let _lease_root = queue_child_dir(queue_dir, "lease")
        .map_err(|_error| SharedQueueClaimError::InvalidQueueDirectory)?;
    let mut jobs = pending_queue_jobs(&pending_dir, &pending_dir_fd)?;
    jobs.sort_by(|left, right| left.0.cmp(&right.0));

    for (job_name, pending_path) in jobs {
        let claim_dir = claimed_root.join(&job_name);
        match nix::sys::stat::mkdirat(
            &claimed_root_fd,
            job_name.as_str(),
            nix::sys::stat::Mode::from_bits_truncate(0o755),
        ) {
            Ok(()) => {}
            Err(nix::errno::Errno::EEXIST) => continue,
            Err(_error) => return Err(SharedQueueClaimError::CannotCreateClaim),
        }
        let claim_dir_fd = match open_queue_entry_dir(&claimed_root_fd, &job_name) {
            Ok(directory) => directory,
            Err(_error) => return Err(SharedQueueClaimError::CannotCreateClaim),
        };

        let claimed_path = claim_dir.join(&job_name);
        match nix::fcntl::renameat(
            &pending_dir_fd,
            job_name.as_str(),
            &claim_dir_fd,
            job_name.as_str(),
        ) {
            Ok(()) => {
                if sync_claimed_queue_job(&pending_dir, &claim_dir, &claimed_root).is_err() {
                    rollback_shared_queue_claim(queue_dir, &job_name, &pending_path, &claim_dir);
                    return Err(SharedQueueClaimError::CannotClaimJob);
                }
                let lease_path = match record_shared_queue_lease(queue_dir, &job_name, worker_name)
                {
                    Ok(lease_path) => lease_path,
                    Err(error) => {
                        rollback_shared_queue_claim(
                            queue_dir,
                            &job_name,
                            &pending_path,
                            &claim_dir,
                        );
                        return Err(error);
                    }
                };
                return Ok(Some(SharedQueueClaim::new(
                    job_name,
                    claimed_path,
                    lease_path,
                )));
            }
            Err(nix::errno::Errno::ENOENT) => {
                let _ignored = nix::unistd::unlinkat(
                    &claimed_root_fd,
                    job_name.as_str(),
                    nix::unistd::UnlinkatFlags::RemoveDir,
                );
            }
            Err(_error) => return Err(SharedQueueClaimError::CannotClaimJob),
        }
    }

    Ok(None)
}

/// Finishes a claimed queue job by materializing a readable result file under
/// `done/` or `failed/`, moving the original claimed request beside it, and
/// removing the recoverable lease.
pub fn finish_shared_queue_job(
    queue_dir: &Path,
    job_name: &str,
    outcome: SharedQueueOutcome,
    result: &[u8],
) -> Result<PathBuf, SharedQueueFinishError> {
    if !is_queue_job_name(job_name) {
        return Err(SharedQueueFinishError::InvalidJobName);
    }

    let output_dir = queue_child_dir(queue_dir, outcome.as_dir())
        .map_err(|_error| SharedQueueFinishError::InvalidQueueDirectory)?;
    let output_dir_fd = queue_child_dir_fd(queue_dir, outcome.as_dir())
        .map_err(|_error| SharedQueueFinishError::InvalidQueueDirectory)?;
    let _claimed_root = queue_child_dir(queue_dir, "claimed")
        .map_err(|_error| SharedQueueFinishError::InvalidQueueDirectory)?;
    let _lease_root = queue_child_dir(queue_dir, "lease")
        .map_err(|_error| SharedQueueFinishError::InvalidQueueDirectory)?;
    let claim_dir_fd = queue_job_plain_dir_fd(queue_dir, "claimed", job_name)
        .map_err(|_error| SharedQueueFinishError::CannotMoveClaimedJob)?;
    let lease_dir_fd = queue_job_plain_dir_fd(queue_dir, "lease", job_name)
        .map_err(|_error| SharedQueueFinishError::CannotMoveClaimedJob)?;
    if !fd_entry_is_plain_file(&claim_dir_fd, job_name)
        || !fd_entry_is_plain_file(&lease_dir_fd, "worker")
    {
        return Err(SharedQueueFinishError::CannotMoveClaimedJob);
    }
    let result_name = format!("{job_name}.result");
    if fd_entry_exists(&output_dir_fd, job_name) || fd_entry_exists(&output_dir_fd, &result_name) {
        return Err(SharedQueueFinishError::CannotWriteResult);
    }

    let result_path = output_dir.join(&result_name);
    write_queue_result_atomic(&output_dir, &output_dir_fd, job_name, &result_name, result)
        .map_err(|_error| SharedQueueFinishError::CannotWriteResult)?;

    nix::fcntl::renameat(&claim_dir_fd, job_name, &output_dir_fd, job_name)
        .map_err(|_error| SharedQueueFinishError::CannotMoveClaimedJob)?;
    crate::plain_fs::sync_plain_dir(&output_dir)
        .map_err(|_error| SharedQueueFinishError::CannotMoveClaimedJob)?;
    claim_dir_fd
        .sync_all()
        .map_err(|_error| SharedQueueFinishError::CannotMoveClaimedJob)?;
    cleanup_shared_queue_claim(queue_dir, job_name)
        .map_err(|_error| SharedQueueFinishError::CannotCleanup)?;

    Ok(result_path)
}

/// Recovers an explicitly abandoned claimed job by moving it back to
/// `pending/`. The existing `lease/<job>/worker` file is the durable evidence
/// that a worker previously claimed the job.
pub fn recover_shared_queue_job(
    queue_dir: &Path,
    job_name: &str,
) -> Result<PathBuf, SharedQueueRecoverError> {
    if !is_queue_job_name(job_name) {
        return Err(SharedQueueRecoverError::InvalidJobName);
    }

    let pending_root = queue_child_dir(queue_dir, "pending")
        .map_err(|_error| SharedQueueRecoverError::InvalidQueueDirectory)?;
    let pending_root_fd = queue_child_dir_fd(queue_dir, "pending")
        .map_err(|_error| SharedQueueRecoverError::InvalidQueueDirectory)?;
    let _claimed_root = queue_child_dir(queue_dir, "claimed")
        .map_err(|_error| SharedQueueRecoverError::InvalidQueueDirectory)?;
    let _lease_root = queue_child_dir(queue_dir, "lease")
        .map_err(|_error| SharedQueueRecoverError::InvalidQueueDirectory)?;
    let claim_dir_fd = match queue_job_plain_dir_fd(queue_dir, "claimed", job_name) {
        Ok(directory) => directory,
        Err(_error) => return Err(SharedQueueRecoverError::MissingClaim),
    };

    if !fd_entry_is_plain_file(&claim_dir_fd, job_name) {
        return Err(SharedQueueRecoverError::MissingClaim);
    }
    let lease_dir_fd = queue_job_plain_dir_fd(queue_dir, "lease", job_name)
        .map_err(|_error| SharedQueueRecoverError::MissingLease)?;
    if !fd_entry_is_plain_file(&lease_dir_fd, "worker") {
        return Err(SharedQueueRecoverError::MissingLease);
    }

    let pending_path = queue_dir.join("pending").join(job_name);
    if path_exists_no_follow(&pending_path) {
        return Err(SharedQueueRecoverError::CannotRequeue);
    }
    nix::fcntl::renameat(&claim_dir_fd, job_name, &pending_root_fd, job_name)
        .map_err(|_error| SharedQueueRecoverError::CannotRequeue)?;
    crate::plain_fs::sync_plain_dir(&pending_root)
        .map_err(|_error| SharedQueueRecoverError::CannotRequeue)?;
    claim_dir_fd
        .sync_all()
        .map_err(|_error| SharedQueueRecoverError::CannotRequeue)?;
    cleanup_shared_queue_claim(queue_dir, job_name)
        .map_err(|_error| SharedQueueRecoverError::CannotCleanup)?;

    Ok(pending_path)
}

pub(crate) fn pending_queue_jobs(
    pending_dir: &Path,
    pending_dir_fd: &fs::File,
) -> Result<Vec<(String, PathBuf)>, SharedQueueClaimError> {
    let entries = fs::read_dir(crate::plain_fs::proc_fd_path(pending_dir_fd))
        .map_err(|_error| SharedQueueClaimError::CannotReadPending)?;
    let mut jobs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_error| SharedQueueClaimError::CannotReadPending)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_queue_job_name(&name) {
            continue;
        }
        let stat = nix::sys::stat::fstatat(
            pending_dir_fd,
            name.as_str(),
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(|_error| SharedQueueClaimError::CannotInspectJob)?;
        if is_regular_mode(stat.st_mode) {
            jobs.push((name.clone(), pending_dir.join(name)));
        }
    }
    Ok(jobs)
}

pub(crate) fn record_shared_queue_lease(
    queue_dir: &Path,
    job_name: &str,
    worker_name: &str,
) -> Result<PathBuf, SharedQueueClaimError> {
    let lease_root = queue_child_dir(queue_dir, "lease")
        .map_err(|_error| SharedQueueClaimError::CannotRecordLease)?;
    let lease_root_fd = queue_child_dir_fd(queue_dir, "lease")
        .map_err(|_error| SharedQueueClaimError::CannotRecordLease)?;
    let lease_path = lease_root.join(job_name);
    nix::sys::stat::mkdirat(
        &lease_root_fd,
        job_name,
        nix::sys::stat::Mode::from_bits_truncate(0o755),
    )
    .map_err(|_error| SharedQueueClaimError::CannotRecordLease)?;
    let lease_dir_fd = open_queue_entry_dir(&lease_root_fd, job_name)
        .map_err(|_error| SharedQueueClaimError::CannotRecordLease)?;
    lease_root_fd
        .sync_all()
        .map_err(|_error| SharedQueueClaimError::CannotRecordLease)?;
    let worker = if worker_name.ends_with('\n') {
        worker_name.to_owned()
    } else {
        format!("{worker_name}\n")
    };
    write_new_file_synced_at(&lease_dir_fd, "worker", worker.as_bytes())
        .map_err(|_error| SharedQueueClaimError::CannotRecordLease)?;
    lease_dir_fd
        .sync_all()
        .map_err(|_error| SharedQueueClaimError::CannotRecordLease)?;
    Ok(lease_path)
}

pub(crate) fn sync_claimed_queue_job(
    pending_dir: &Path,
    claim_dir: &Path,
    claimed_root: &Path,
) -> io::Result<()> {
    crate::plain_fs::sync_plain_dir(pending_dir)?;
    crate::plain_fs::sync_plain_dir(claim_dir)?;
    crate::plain_fs::sync_plain_dir(claimed_root)
}

pub(crate) fn rollback_shared_queue_claim(
    queue_dir: &Path,
    job_name: &str,
    pending_path: &Path,
    claim_dir: &Path,
) {
    if let (Ok(claim_dir_fd), Ok(pending_dir_fd)) = (
        queue_job_plain_dir_fd(queue_dir, "claimed", job_name),
        queue_child_dir_fd(queue_dir, "pending"),
    ) {
        let _ignored = nix::fcntl::renameat(&claim_dir_fd, job_name, &pending_dir_fd, job_name);
    }
    if let Some(parent) = pending_path.parent() {
        let _ignored = crate::plain_fs::sync_plain_dir(parent);
    }
    if let Ok(claimed_root_fd) = queue_child_dir_fd(queue_dir, "claimed") {
        let _ignored = nix::unistd::unlinkat(
            &claimed_root_fd,
            job_name,
            nix::unistd::UnlinkatFlags::RemoveDir,
        );
    }
    if let Some(parent) = claim_dir.parent() {
        let _ignored = crate::plain_fs::sync_plain_dir(parent);
    }
    let _ignored = remove_shared_queue_lease(queue_dir, job_name);
}

pub(crate) fn cleanup_shared_queue_claim(queue_dir: &Path, job_name: &str) -> io::Result<()> {
    let claimed_root = queue_child_dir(queue_dir, "claimed")?;
    let claimed_root_fd = queue_child_dir_fd(queue_dir, "claimed")?;
    match nix::unistd::unlinkat(
        &claimed_root_fd,
        job_name,
        nix::unistd::UnlinkatFlags::RemoveDir,
    ) {
        Ok(()) | Err(nix::errno::Errno::ENOENT) => {}
        Err(error) => return Err(io::Error::from(error)),
    }
    crate::plain_fs::sync_plain_dir(&claimed_root)?;
    remove_shared_queue_lease(queue_dir, job_name)
}

pub(crate) fn remove_shared_queue_lease(queue_dir: &Path, job_name: &str) -> io::Result<()> {
    let _lease_root = queue_child_dir(queue_dir, "lease")?;
    let lease_root_fd = queue_child_dir_fd(queue_dir, "lease")?;
    let lease_dir = match open_queue_entry_dir(&lease_root_fd, job_name) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };

    match nix::sys::stat::fstatat(
        &lease_dir,
        "worker",
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    ) {
        Ok(stat) if is_regular_mode(stat.st_mode) => {
            nix::unistd::unlinkat(
                &lease_dir,
                "worker",
                nix::unistd::UnlinkatFlags::NoRemoveDir,
            )
            .map_err(io::Error::from)?;
        }
        Ok(_stat) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "queue lease worker is not a plain file",
            ));
        }
        Err(nix::errno::Errno::ENOENT) => {}
        Err(error) => return Err(io::Error::from(error)),
    }
    match nix::unistd::unlinkat(
        &lease_root_fd,
        job_name,
        nix::unistd::UnlinkatFlags::RemoveDir,
    ) {
        Ok(()) | Err(nix::errno::Errno::ENOENT) => {}
        Err(error) => return Err(io::Error::from(error)),
    }
    lease_root_fd.sync_all()?;
    Ok(())
}

pub mod store;
use shared_queue_io::*;
pub use store as shared_queue_io;
