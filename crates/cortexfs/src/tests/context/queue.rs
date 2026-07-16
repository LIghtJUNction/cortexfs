use super::*;

#[test]
fn shared_queue_layout_inspector_checks_recommended_dirs() {
    let root = clean_test_dir("shared-queue-layout");
    for dir in SHARED_QUEUE_REQUIRED_DIRS {
        assert!(fs::create_dir_all(root.join(dir)).is_ok());
    }
    assert!(inspect_shared_queue_layout(&root).is_ok());

    assert!(fs::remove_dir_all(root.join("failed")).is_ok());
    assert!(fs::remove_dir_all(root.join("done")).is_ok());
    assert!(fs::write(root.join("done"), "not a dir\n").is_ok());
    let issues = inspect_shared_queue_layout(&root);
    assert!(issues.issues().contains(&PathLayoutIssue::missing(
        "failed".to_owned(),
        LayoutPathRole::Directory
    )));
    assert!(issues.issues().contains(&PathLayoutIssue::wrong_kind(
        "done".to_owned(),
        LayoutPathRole::Directory
    )));
}

#[test]
fn shared_queue_layout_rejects_symlink_directories() {
    let root = clean_test_dir("shared-queue-layout-symlink");
    for dir in SHARED_QUEUE_REQUIRED_DIRS {
        assert!(fs::create_dir_all(root.join(dir)).is_ok());
    }
    let outside = clean_test_dir("shared-queue-layout-symlink-outside");
    assert!(fs::remove_dir_all(root.join("pending")).is_ok());
    assert!(symlink(&outside, root.join("pending")).is_ok());

    assert!(
        inspect_shared_queue_layout(&root)
            .issues()
            .contains(&PathLayoutIssue::wrong_kind(
                "pending".to_owned(),
                LayoutPathRole::Directory
            ))
    );
}
