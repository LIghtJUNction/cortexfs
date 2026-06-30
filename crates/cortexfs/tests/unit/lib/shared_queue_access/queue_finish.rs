#[test]
fn shared_queue_finish_writes_readable_done_result_and_cleans_lease() {
    let root = clean_test_dir("shared-queue-finish-done");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");

    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    let result_path =
        finish_shared_queue_job(&root, claimed.job_name(), SharedQueueOutcome::Done, b"ok\n");
    assert_eq!(
        result_path,
        Ok(root.join("done").join("job-1.req.json.result"))
    );
    assert_file_text(&root.join("done").join("job-1.req.json.result"), "ok\n");
    let mode = fs::metadata(root.join("done").join("job-1.req.json.result"))
        .map(|metadata| metadata.mode() & 0o777);
    assert!(matches!(mode, Ok(0o644)));
    let temp_leftovers = fs::read_dir(root.join("done"))
        .map_or(usize::MAX, |entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .contains(".result.tmp-")
                })
                .count()
        });
    assert_eq!(temp_leftovers, 0);
    assert_file_text(&root.join("done").join("job-1.req.json"), "one\n");
    assert!(!root.join("claimed").join("job-1.req.json").exists());
    assert!(!root.join("lease").join("job-1.req.json").exists());
}

#[test]
fn shared_queue_finish_writes_readable_failed_result() {
    let root = clean_test_dir("shared-queue-finish-failed");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");

    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    let result_path = finish_shared_queue_job(
        &root,
        claimed.job_name(),
        SharedQueueOutcome::Failed,
        b"err\n",
    );
    assert_eq!(
        result_path,
        Ok(root.join("failed").join("job-1.req.json.result"))
    );
    assert_file_text(&root.join("failed").join("job-1.req.json.result"), "err\n");
    let mode = fs::metadata(root.join("failed").join("job-1.req.json.result"))
        .map(|metadata| metadata.mode() & 0o777);
    assert!(matches!(mode, Ok(0o644)));
    assert_file_text(&root.join("failed").join("job-1.req.json"), "one\n");
}

#[test]
fn shared_queue_finish_does_not_overwrite_existing_result() {
    let root = clean_test_dir("shared-queue-finish-existing-result");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");

    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    write_text_file(&root.join("done").join("job-1.req.json.result"), "old\n");

    let result = finish_shared_queue_job(
        &root,
        claimed.job_name(),
        SharedQueueOutcome::Done,
        b"new\n",
    );

    assert_eq!(result, Err(SharedQueueFinishError::CannotWriteResult));
    assert_file_text(&root.join("done").join("job-1.req.json.result"), "old\n");
    assert_file_text(
        &root
            .join("claimed")
            .join("job-1.req.json")
            .join("job-1.req.json"),
        "one\n",
    );
    assert_file_text(&root.join("lease").join("job-1.req.json").join("worker"), "worker-a\n");
}
