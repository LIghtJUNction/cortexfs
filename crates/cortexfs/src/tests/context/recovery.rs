#[test]
fn shared_queue_recovery_refuses_to_overwrite_pending_job() {
    let root = clean_test_dir("shared-queue-recover-no-overwrite");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    write_text_file(&root.join("pending").join("job-1.req.json"), "new\n");

    assert_eq!(
        recover_shared_queue_job(&root, claimed.job_name()),
        Err(SharedQueueRecoverError::CannotRequeue)
    );
    assert_file_text(&root.join("pending").join("job-1.req.json"), "new\n");
    assert!(claimed.claimed_path().exists());
}

#[test]
fn shared_queue_finish_rejects_symlink_output_directory_without_writing_target() {
    let root = clean_test_dir("shared-queue-finish-symlink-dir");
    let outside = clean_test_dir("shared-queue-finish-symlink-dir-outside");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    assert!(fs::remove_dir_all(root.join("done")).is_ok());
    assert!(symlink(&outside, root.join("done")).is_ok());

    assert_eq!(
        finish_shared_queue_job(&root, claimed.job_name(), SharedQueueOutcome::Done, b"ok\n"),
        Err(SharedQueueFinishError::InvalidQueueDirectory)
    );
    assert!(!outside.join("job-1.req.json.result").exists());
}

#[test]
fn shared_queue_finish_rejects_symlink_lease_directory_without_touching_target() {
    let root = clean_test_dir("shared-queue-finish-symlink-lease");
    let outside = clean_test_dir("shared-queue-finish-symlink-lease-outside");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    assert!(fs::remove_dir_all(root.join("lease").join(claimed.job_name())).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    write_text_file(&outside.join("worker"), "worker-a\n");
    assert!(symlink(&outside, root.join("lease").join(claimed.job_name())).is_ok());

    assert_eq!(
        finish_shared_queue_job(&root, claimed.job_name(), SharedQueueOutcome::Done, b"ok\n"),
        Err(SharedQueueFinishError::CannotMoveClaimedJob)
    );
    assert_file_text(&outside.join("worker"), "worker-a\n");
    assert!(!root.join("done").join(claimed.job_name()).exists());
    assert!(
        !root
            .join("done")
            .join(format!("{}.result", claimed.job_name()))
            .exists()
    );
}

#[test]
fn shared_queue_finish_rejects_symlink_claim_directory_without_touching_target() {
    let root = clean_test_dir("shared-queue-finish-symlink-claim");
    let outside = clean_test_dir("shared-queue-finish-symlink-claim-outside");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    assert!(fs::remove_dir_all(root.join("claimed").join(claimed.job_name())).is_ok());
    write_text_file(&outside.join(claimed.job_name()), "outside\n");
    assert!(symlink(&outside, root.join("claimed").join(claimed.job_name())).is_ok());

    assert_eq!(
        finish_shared_queue_job(&root, claimed.job_name(), SharedQueueOutcome::Done, b"ok\n"),
        Err(SharedQueueFinishError::CannotMoveClaimedJob)
    );
    assert_file_text(&outside.join(claimed.job_name()), "outside\n");
    assert!(!root.join("done").join(claimed.job_name()).exists());
    assert!(
        !root
            .join("done")
            .join(format!("{}.result", claimed.job_name()))
            .exists()
    );
}

#[test]
fn shared_queue_recovery_requires_existing_claim_and_lease() {
    let root = clean_test_dir("shared-queue-recover-missing");
    create_shared_queue_layout(&root);
    assert_eq!(
        recover_shared_queue_job(&root, "job-1.req.json"),
        Err(SharedQueueRecoverError::MissingClaim)
    );

    let claim_dir = root.join("claimed").join("job-1.req.json");
    assert!(fs::create_dir_all(&claim_dir).is_ok());
    write_text_file(&claim_dir.join("job-1.req.json"), "one\n");
    assert_eq!(
        recover_shared_queue_job(&root, "job-1.req.json"),
        Err(SharedQueueRecoverError::MissingLease)
    );
}

#[test]
fn shared_queue_recovery_rejects_symlink_lease_directory_without_touching_target() {
    let root = clean_test_dir("shared-queue-recover-symlink-lease");
    let outside = clean_test_dir("shared-queue-recover-symlink-lease-outside");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    assert!(fs::remove_dir_all(root.join("lease").join(claimed.job_name())).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    write_text_file(&outside.join("worker"), "worker-a\n");
    assert!(symlink(&outside, root.join("lease").join(claimed.job_name())).is_ok());

    assert_eq!(
        recover_shared_queue_job(&root, claimed.job_name()),
        Err(SharedQueueRecoverError::MissingLease)
    );
    assert_file_text(&outside.join("worker"), "worker-a\n");
    assert!(!root.join("pending").join(claimed.job_name()).exists());
    assert!(claimed.claimed_path().exists());
}

#[test]
fn shared_queue_recovery_rejects_symlink_claim_directory_without_touching_target() {
    let root = clean_test_dir("shared-queue-recover-symlink-claim");
    let outside = clean_test_dir("shared-queue-recover-symlink-claim-outside");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    assert!(fs::remove_dir_all(root.join("claimed").join(claimed.job_name())).is_ok());
    write_text_file(&outside.join(claimed.job_name()), "outside\n");
    assert!(symlink(&outside, root.join("claimed").join(claimed.job_name())).is_ok());

    assert_eq!(
        recover_shared_queue_job(&root, claimed.job_name()),
        Err(SharedQueueRecoverError::MissingClaim)
    );
    assert_file_text(&outside.join(claimed.job_name()), "outside\n");
    assert!(!root.join("pending").join(claimed.job_name()).exists());
}
use super::*;
