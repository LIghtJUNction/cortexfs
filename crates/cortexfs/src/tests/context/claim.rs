#[test]
fn shared_queue_layout_inspector_checks_recommended_dirs() {
    let root = clean_test_dir("shared-queue-layout");
    for dir in SHARED_QUEUE_REQUIRED_DIRS {
        assert!(fs::create_dir_all(root.join(dir)).is_ok());
    }
    let report = inspect_shared_queue_layout(&root);
    assert!(report.is_ok());

    assert!(fs::remove_dir_all(root.join("failed")).is_ok());
    assert!(fs::remove_dir_all(root.join("done")).is_ok());
    assert!(fs::write(root.join("done"), "not a dir\n").is_ok());
    let report = inspect_shared_queue_layout(&root);
    assert!(!report.is_ok());
    assert!(report.issues().contains(&PathLayoutIssue::missing(
        "failed".to_owned(),
        LayoutPathRole::Directory
    )));
    assert!(report.issues().contains(&PathLayoutIssue::wrong_kind(
        "done".to_owned(),
        LayoutPathRole::Directory
    )));
}

#[test]
fn shared_queue_layout_rejects_symlink_directories() {
    let root = clean_test_dir("shared-queue-layout-symlink");
    create_shared_queue_layout(&root);
    let outside = clean_test_dir("shared-queue-layout-symlink-outside");
    assert!(fs::remove_dir_all(root.join("pending")).is_ok());
    assert!(symlink(&outside, root.join("pending")).is_ok());

    let report = inspect_shared_queue_layout(&root);
    assert!(report.issues().contains(&PathLayoutIssue::wrong_kind(
        "pending".to_owned(),
        LayoutPathRole::Directory
    )));
    assert_eq!(
        claim_next_shared_queue_job(&root, "worker-a"),
        Err(SharedQueueClaimError::InvalidQueueDirectory)
    );
}

#[test]
fn shared_queue_claim_uses_atomic_claim_directories() {
    let root = clean_test_dir("shared-queue-claim");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-2.req.json"), "two\n");
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    write_text_file(&root.join("pending").join(".ignored"), "bad\n");
    assert!(fs::create_dir_all(root.join("pending").join("not-file")).is_ok());

    let first = claim_next_shared_queue_job(&root, "worker-a");
    let Some(first) = ok!(first) else { return };
    assert_eq!(first.job_name(), "job-1.req.json");
    assert_file_text(first.claimed_path(), "one\n");
    assert_file_text(&first.lease_path().join("worker"), "worker-a\n");
    assert!(!root.join("pending").join("job-1.req.json").exists());

    let second = claim_next_shared_queue_job(&root, "worker-b");
    let Some(second) = ok!(second) else { return };
    assert_eq!(second.job_name(), "job-2.req.json");

    let none = claim_next_shared_queue_job(&root, "worker-c");
    assert_eq!(none, Ok(None));
}

#[test]
fn shared_queue_claim_ignores_non_req_json_and_symlink_jobs() {
    let root = clean_test_dir("shared-queue-claim-ignore");
    create_shared_queue_layout(&root);
    let outside = clean_test_dir("shared-queue-claim-ignore-outside");
    write_text_file(&outside.join("secret.txt"), "secret\n");
    assert!(
        symlink(
            outside.join("secret.txt"),
            root.join("pending").join("job-0.req.json")
        )
        .is_ok()
    );
    write_text_file(&root.join("pending").join("job-1.req.json.tmp"), "tmp\n");
    write_text_file(&root.join("pending").join("job-2.req.json"), "real\n");

    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    assert_eq!(claimed.job_name(), "job-2.req.json");
    assert!(root.join("pending").join("job-0.req.json").exists());
    assert!(root.join("pending").join("job-1.req.json.tmp").exists());
}

#[test]
fn shared_queue_claim_skips_existing_claim_lock() {
    let root = clean_test_dir("shared-queue-claim-lock");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    write_text_file(&root.join("pending").join("job-2.req.json"), "two\n");
    assert!(fs::create_dir_all(root.join("claimed").join("job-1.req.json")).is_ok());

    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    assert_eq!(claimed.job_name(), "job-2.req.json");
    assert!(root.join("pending").join("job-1.req.json").exists());
}

#[test]
fn shared_queue_claim_rolls_back_when_lease_recording_fails() {
    let root = clean_test_dir("shared-queue-claim-lease-fail");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    assert!(fs::write(root.join("lease").join("job-1.req.json"), "not a dir\n").is_ok());

    assert_eq!(
        claim_next_shared_queue_job(&root, "worker-a"),
        Err(SharedQueueClaimError::CannotRecordLease)
    );
    assert_file_text(&root.join("pending").join("job-1.req.json"), "one\n");
    assert!(!root.join("claimed").join("job-1.req.json").exists());
}

#[test]
fn shared_queue_claim_rejects_symlink_queue_root_without_touching_target() {
    let root = clean_test_dir("shared-queue-claim-symlink-root");
    let outside = clean_test_dir("shared-queue-claim-symlink-root-outside");
    create_shared_queue_layout(&outside);
    write_text_file(&outside.join("pending").join("job-1.req.json"), "one\n");
    assert!(symlink(&outside, &root).is_ok());

    assert_eq!(
        claim_next_shared_queue_job(&root, "worker-a"),
        Err(SharedQueueClaimError::InvalidQueueDirectory)
    );
    assert_file_text(&outside.join("pending").join("job-1.req.json"), "one\n");
    assert!(!outside.join("claimed").join("job-1.req.json").exists());
}

#[test]
fn shared_queue_finish_and_recover_reject_non_request_job_names() {
    let root = clean_test_dir("shared-queue-invalid-job-name");
    create_shared_queue_layout(&root);

    assert_eq!(
        finish_shared_queue_job(&root, "job-1", SharedQueueOutcome::Done, b"ok\n"),
        Err(SharedQueueFinishError::InvalidJobName)
    );
    assert_eq!(
        finish_shared_queue_job(
            &root,
            "job-1.req.json.tmp",
            SharedQueueOutcome::Done,
            b"ok\n"
        ),
        Err(SharedQueueFinishError::InvalidJobName)
    );
    assert_eq!(
        recover_shared_queue_job(&root, "job-1"),
        Err(SharedQueueRecoverError::InvalidJobName)
    );
    assert_eq!(
        recover_shared_queue_job(&root, "job-1.req.json.tmp"),
        Err(SharedQueueRecoverError::InvalidJobName)
    );
}

#[test]
fn shared_queue_recovery_requeues_claimed_job_with_lease() {
    let root = clean_test_dir("shared-queue-recover");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");

    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    assert!(claimed.claimed_path().is_file());
    assert!(claimed.lease_path().join("worker").is_file());

    let recovered = recover_shared_queue_job(&root, "job-1.req.json");
    assert_eq!(recovered, Ok(root.join("pending").join("job-1.req.json")));
    assert_file_text(&root.join("pending").join("job-1.req.json"), "one\n");
    assert!(!root.join("claimed").join("job-1.req.json").exists());
    assert!(!root.join("lease").join("job-1.req.json").exists());
}

#[test]
fn shared_queue_finish_does_not_write_result_without_claim() {
    let root = clean_test_dir("shared-queue-finish-without-claim");
    create_shared_queue_layout(&root);

    assert_eq!(
        finish_shared_queue_job(&root, "job-1.req.json", SharedQueueOutcome::Done, b"ok\n"),
        Err(SharedQueueFinishError::CannotMoveClaimedJob)
    );
    assert!(!root.join("done").join("job-1.req.json.result").exists());
}

#[test]
fn shared_queue_finish_refuses_to_overwrite_output_entries() {
    let root = clean_test_dir("shared-queue-finish-no-overwrite");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    write_text_file(&root.join("done").join("job-1.req.json.result"), "old\n");

    assert_eq!(
        finish_shared_queue_job(
            &root,
            claimed.job_name(),
            SharedQueueOutcome::Done,
            b"new\n"
        ),
        Err(SharedQueueFinishError::CannotWriteResult)
    );
    assert_file_text(&root.join("done").join("job-1.req.json.result"), "old\n");
    assert!(claimed.claimed_path().exists());

    assert!(fs::remove_file(root.join("done").join("job-1.req.json.result")).is_ok());
    write_text_file(&root.join("done").join("job-1.req.json"), "old request\n");
    assert_eq!(
        finish_shared_queue_job(
            &root,
            claimed.job_name(),
            SharedQueueOutcome::Done,
            b"new\n"
        ),
        Err(SharedQueueFinishError::CannotWriteResult)
    );
    assert_file_text(&root.join("done").join("job-1.req.json"), "old request\n");
    assert!(!root.join("done").join("job-1.req.json.result").exists());
}

#[test]
fn shared_queue_finish_refuses_symlink_result_without_writing_target() {
    let root = clean_test_dir("shared-queue-finish-symlink-result");
    let outside = clean_test_dir("shared-queue-finish-symlink-result-outside");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    write_text_file(&outside.join("result"), "outside\n");
    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    assert!(
        symlink(
            outside.join("result"),
            root.join("done").join("job-1.req.json.result")
        )
        .is_ok()
    );

    assert_eq!(
        finish_shared_queue_job(
            &root,
            claimed.job_name(),
            SharedQueueOutcome::Done,
            b"new\n"
        ),
        Err(SharedQueueFinishError::CannotWriteResult)
    );
    assert_file_text(&outside.join("result"), "outside\n");
    assert!(claimed.claimed_path().exists());
}
use super::*;
