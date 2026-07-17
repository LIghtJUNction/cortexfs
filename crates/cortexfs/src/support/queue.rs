use std::path::Path;

use crate::support::layout::require_symlink_dir;
use crate::{PathLayoutIssue, SHARED_QUEUE_REQUIRED_DIRS};

/// Shared queue layout uses the shared path-layout issue model.
pub type SharedQueueLayoutIssue = PathLayoutIssue;

/// Result of inspecting a shared queue directory.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SharedQueueLayoutReport {
    issues: Vec<PathLayoutIssue>,
}

impl_issue_report!(SharedQueueLayoutReport, PathLayoutIssue);

/// Inspects a shared project queue for the recommended directory shape.
#[must_use]
pub fn inspect_shared_queue_layout(queue_dir: &Path) -> SharedQueueLayoutReport {
    let mut issues = Vec::new();
    for dir in SHARED_QUEUE_REQUIRED_DIRS {
        require_symlink_dir(&queue_dir.join(dir), dir, &mut issues);
    }
    SharedQueueLayoutReport::new(issues)
}
