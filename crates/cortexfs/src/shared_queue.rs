use std::fs;
use std::path::{Path, PathBuf};

use crate::{SHARED_QUEUE_REQUIRED_DIRS, is_object_name};

/// Shared queue layout validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SharedQueueLayoutIssue {
    /// Required directory is missing.
    MissingDirectory(String),
    /// Path exists but is not a directory.
    NotDirectory(String),
}

/// Result of inspecting a shared queue directory.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SharedQueueLayoutReport {
    issues: Vec<SharedQueueLayoutIssue>,
}

impl SharedQueueLayoutReport {
    /// Creates a report with collected layout issues.
    #[must_use]
    pub const fn new(issues: Vec<SharedQueueLayoutIssue>) -> Self {
        Self { issues }
    }

    /// Returns true when the queue satisfies the v1 layout.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns all detected layout issues.
    #[must_use]
    pub fn issues(&self) -> &[SharedQueueLayoutIssue] {
        &self.issues
    }
}

/// Error while claiming a shared queue job.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedQueueClaimError {
    /// Worker name is not a valid object-like token.
    InvalidWorkerName,
    /// Queue `pending/` cannot be read.
    CannotReadPending,
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
    /// Job name is not a valid object-like token.
    InvalidJobName,
    /// Result could not be written into `done/` or `failed/`.
    CannotWriteResult,
    /// Claimed job file could not be moved into `done/` or `failed/`.
    CannotMoveClaimedJob,
    /// Claim or lease cleanup failed.
    CannotCleanup,
}

/// Error while recovering a shared queue job claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedQueueRecoverError {
    /// Job name is not a valid object-like token.
    InvalidJobName,
    /// No claimed job file exists to recover.
    MissingClaim,
    /// No lease exists for the claimed job.
    MissingLease,
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
        require_shared_queue_directory(&queue_dir.join(dir), dir, &mut issues);
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

    let mut jobs = pending_queue_jobs(&queue_dir.join("pending"))?;
    jobs.sort_by(|left, right| left.0.cmp(&right.0));

    for (job_name, pending_path) in jobs {
        let claim_dir = queue_dir.join("claimed").join(&job_name);
        match fs::DirBuilder::new().create(&claim_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_error) => return Err(SharedQueueClaimError::CannotCreateClaim),
        }

        let claimed_path = claim_dir.join(&job_name);
        match fs::rename(&pending_path, &claimed_path) {
            Ok(()) => {
                let lease_path = record_shared_queue_lease(queue_dir, &job_name, worker_name)?;
                return Ok(Some(SharedQueueClaim::new(
                    job_name,
                    claimed_path,
                    lease_path,
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let _ignored = fs::remove_dir(&claim_dir);
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
    if !is_object_name(job_name) {
        return Err(SharedQueueFinishError::InvalidJobName);
    }

    let output_dir = queue_dir.join(outcome.as_dir());
    let result_path = output_dir.join(format!("{job_name}.result"));
    let temp_path = output_dir.join(format!(".{job_name}.result.tmp"));
    fs::write(&temp_path, result).map_err(|_error| SharedQueueFinishError::CannotWriteResult)?;
    fs::rename(&temp_path, &result_path)
        .map_err(|_error| SharedQueueFinishError::CannotWriteResult)?;

    let claimed_file = claimed_queue_job_path(queue_dir, job_name);
    fs::rename(&claimed_file, output_dir.join(job_name))
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
    if !is_object_name(job_name) {
        return Err(SharedQueueRecoverError::InvalidJobName);
    }

    let claimed_file = claimed_queue_job_path(queue_dir, job_name);
    if !claimed_file.is_file() {
        return Err(SharedQueueRecoverError::MissingClaim);
    }
    if !queue_dir
        .join("lease")
        .join(job_name)
        .join("worker")
        .is_file()
    {
        return Err(SharedQueueRecoverError::MissingLease);
    }

    let pending_path = queue_dir.join("pending").join(job_name);
    fs::rename(&claimed_file, &pending_path)
        .map_err(|_error| SharedQueueRecoverError::CannotRequeue)?;
    cleanup_shared_queue_claim(queue_dir, job_name)
        .map_err(|_error| SharedQueueRecoverError::CannotCleanup)?;

    Ok(pending_path)
}

fn pending_queue_jobs(pending_dir: &Path) -> Result<Vec<(String, PathBuf)>, SharedQueueClaimError> {
    let entries =
        fs::read_dir(pending_dir).map_err(|_error| SharedQueueClaimError::CannotReadPending)?;
    let mut jobs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_error| SharedQueueClaimError::CannotReadPending)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_object_name(&name) {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|_error| SharedQueueClaimError::CannotInspectJob)?;
        if metadata.is_file() {
            jobs.push((name, entry.path()));
        }
    }
    Ok(jobs)
}

fn record_shared_queue_lease(
    queue_dir: &Path,
    job_name: &str,
    worker_name: &str,
) -> Result<PathBuf, SharedQueueClaimError> {
    let lease_path = queue_dir.join("lease").join(job_name);
    fs::create_dir_all(&lease_path).map_err(|_error| SharedQueueClaimError::CannotRecordLease)?;
    fs::write(lease_path.join("worker"), newline_terminated(worker_name))
        .map_err(|_error| SharedQueueClaimError::CannotRecordLease)?;
    Ok(lease_path)
}

fn claimed_queue_job_path(queue_dir: &Path, job_name: &str) -> PathBuf {
    queue_dir.join("claimed").join(job_name).join(job_name)
}

fn cleanup_shared_queue_claim(queue_dir: &Path, job_name: &str) -> std::io::Result<()> {
    let claim_dir = queue_dir.join("claimed").join(job_name);
    let lease_dir = queue_dir.join("lease").join(job_name);
    match fs::remove_dir(&claim_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    match fs::remove_dir_all(&lease_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(())
}

fn require_shared_queue_directory(
    path: &Path,
    label: &str,
    issues: &mut Vec<SharedQueueLayoutIssue>,
) {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_metadata) => issues.push(SharedQueueLayoutIssue::NotDirectory(label.to_owned())),
        Err(_error) => issues.push(SharedQueueLayoutIssue::MissingDirectory(label.to_owned())),
    }
}

fn newline_terminated(value: &str) -> String {
    if value.ends_with('\n') {
        value.to_owned()
    } else {
        format!("{value}\n")
    }
}
