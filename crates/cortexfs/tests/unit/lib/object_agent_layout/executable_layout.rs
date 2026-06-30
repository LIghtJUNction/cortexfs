#[test]
fn executable_object_bootstrap_validates_controls_and_agent_socket_boundary() {
    let root = clean_test_dir("object-bootstrap-bad");
    let target = root.join("runtime").join("agent");

    write_fixture_file(&target, 0o755);

    assert_eq!(
        install_executable_object_wrapper(
            &root,
            ObjectClass::Tool,
            "bad/name",
            &target.display().to_string(),
            &[],
        ),
        Err(ObjectBootstrapError::InvalidObjectName)
    );
    assert_eq!(
        install_executable_object_wrapper(&root, ObjectClass::Tool, "fs.read", "bad\ncmd", &[]),
        Err(ObjectBootstrapError::InvalidWrapperTarget)
    );
    assert_eq!(
        install_executable_object_wrapper(&root, ObjectClass::Tool, "fs.read", "bad\u{1b}cmd", &[]),
        Err(ObjectBootstrapError::InvalidWrapperTarget)
    );
    assert_eq!(
        install_executable_object_wrapper(
            &root,
            ObjectClass::Tool,
            "fs.read",
            &target.display().to_string(),
            &[("authority", "root")],
        ),
        Err(ObjectBootstrapError::InvalidControlFile)
    );
    assert_eq!(
        install_executable_object_wrapper(
            &root,
            ObjectClass::Tool,
            "fs.read",
            &target.display().to_string(),
            &[("schema", "{\"authority\":\"root\"}")],
        ),
        Err(ObjectBootstrapError::InvalidControlValue)
    );

    let agent = install_executable_object_wrapper(
        &root,
        ObjectClass::Agent,
        "coder",
        &target.display().to_string(),
        &[("uid", "1000"), ("gid", "1000"), ("owner", "1000")],
    );
    assert!(agent.is_ok());
    let report = inspect_object_layout(&root, ObjectClass::Agent, "coder");
    assert!(!report.is_ok());
    assert!(report.issues().contains(&ObjectLayoutIssue::MissingSocket(
        "agent/coder.sock".to_owned()
    )));
    let _agent_socket = bind_socket(&root.join("agent").join("coder.sock"));
    assert!(inspect_object_layout(&root, ObjectClass::Agent, "coder").is_ok());
    assert_eq!(ObjectBootstrapError::InvalidControlValue.errno(), "EINVAL");
}

#[test]
fn executable_object_bootstrap_rejects_symlink_class_directory() {
    let root = clean_test_dir("object-bootstrap-symlink-class");
    let outside = clean_test_dir("object-bootstrap-symlink-class-outside");
    let target = root.join("runtime").join("tool");
    write_fixture_file(&target, 0o755);
    assert!(symlink(&outside, root.join("tool")).is_ok());

    assert_eq!(
        install_executable_object_wrapper(
            &root,
            ObjectClass::Tool,
            "fs.read",
            &target.display().to_string(),
            &[],
        ),
        Err(ObjectBootstrapError::CannotCreate)
    );
    assert!(!outside.join("fs.read").exists());
    assert!(!outside.join("fs.read.d").exists());
}

#[test]
fn executable_object_bootstrap_rejects_symlink_root_parent_without_writing_target() {
    let root = clean_test_dir("object-bootstrap-symlink-root-parent");
    let outside = clean_test_dir("object-bootstrap-symlink-root-parent-outside");
    let link_root = root.join("ctx-root");
    let target = root.join("runtime").join("tool");
    write_fixture_file(&target, 0o755);
    assert!(fs::create_dir_all(outside.join("tool")).is_ok());
    assert!(symlink(&outside, &link_root).is_ok());

    assert_eq!(
        install_executable_object_wrapper(
            &link_root,
            ObjectClass::Tool,
            "fs.read",
            &target.display().to_string(),
            &[],
        ),
        Err(ObjectBootstrapError::CannotCreate)
    );
    assert!(!outside.join("tool").join("fs.read").exists());
    assert!(!outside.join("tool").join("fs.read.d").exists());
}

#[test]
fn executable_object_bootstrap_rejects_symlink_control_directory() {
    let root = clean_test_dir("object-bootstrap-symlink-control");
    let outside = clean_test_dir("object-bootstrap-symlink-control-outside");
    let target = root.join("runtime").join("tool");
    write_fixture_file(&target, 0o755);
    assert!(fs::create_dir_all(root.join("tool")).is_ok());
    assert!(symlink(&outside, root.join("tool").join("fs.read.d")).is_ok());

    assert_eq!(
        install_executable_object_wrapper(
            &root,
            ObjectClass::Tool,
            "fs.read",
            &target.display().to_string(),
            &[],
        ),
        Err(ObjectBootstrapError::CannotCreate)
    );
    assert!(!outside.join("name").exists());
    assert!(!outside.join("schema").exists());
}

#[test]
fn object_layout_accepts_socket_symlink_to_live_unix_socket() {
    let root = clean_test_dir("object-layout-socket-symlink");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let runtime_socket = root.join("runtime").join("coder.sock");
    let _listener = bind_socket(&runtime_socket);
    assert!(symlink(runtime_socket, root.join("agent").join("coder.sock")).is_ok());

    assert!(inspect_object_layout(&root, ObjectClass::Agent, "coder").is_ok());
}

#[test]
fn object_layout_rejects_symlink_class_directory_for_socket_lookup() {
    let root = clean_test_dir("object-layout-socket-symlink-class");
    let outside = clean_test_dir("object-layout-socket-symlink-class-outside");
    create_complete_object_layout(&outside, ObjectClass::Agent, "coder", "none");
    let _listener = bind_socket(&outside.join("agent").join("coder.sock"));
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(symlink(outside.join("agent"), root.join("agent")).is_ok());

    let report = inspect_object_layout(&root, ObjectClass::Agent, "coder");
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::MissingSocket(
            "agent/coder.sock".to_owned()
        )));
}

#[test]
fn object_layout_reports_missing_parts() {
    let root = clean_test_dir("object-layout-bad");
    assert!(fs::create_dir_all(root.join("agent")).is_ok());
    write_text_file(&root.join("agent").join("coder"), "#!/bin/sh\n");

    let report = inspect_object_layout(&root, ObjectClass::Agent, "coder");
    assert!(!report.is_ok());
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::NotExecutable("agent/coder".to_owned())));
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::MissingControlDirectory(
            "agent/coder.d".to_owned()
        )));
    assert!(report.issues().contains(&ObjectLayoutIssue::MissingSocket(
        "agent/coder.sock".to_owned()
    )));
}

#[test]
fn object_layout_rejects_symlink_executable_and_control_paths() {
    let root = clean_test_dir("object-layout-symlink-controls");
    create_complete_object_layout(&root, ObjectClass::Tool, "fs.read", "");
    let outside = root.join("outside");
    assert!(fs::create_dir_all(&outside).is_ok());
    write_fixture_file(&outside.join("tool-target"), 0o755);
    write_text_file(&outside.join("schema"), "{\"authority\":\"root\"}\n");

    assert!(fs::remove_file(root.join("tool").join("fs.read")).is_ok());
    assert!(symlink(outside.join("tool-target"), root.join("tool").join("fs.read")).is_ok());
    let report = inspect_object_layout(&root, ObjectClass::Tool, "fs.read");
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::NotExecutable("tool/fs.read".to_owned())));

    write_fixture_file(&root.join("tool").join("fs.read"), 0o755);
    assert!(fs::remove_dir_all(root.join("tool").join("fs.read.d")).is_ok());
    assert!(fs::create_dir_all(outside.join("fs.read.d")).is_ok());
    assert!(symlink(outside.join("fs.read.d"), root.join("tool").join("fs.read.d")).is_ok());
    let report = inspect_object_layout(&root, ObjectClass::Tool, "fs.read");
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::NotControlDirectory(
            "tool/fs.read.d".to_owned()
        )));

    assert!(fs::remove_file(root.join("tool").join("fs.read.d")).is_ok());
    create_complete_object_layout(&root, ObjectClass::Tool, "fs.read", "");
    assert!(fs::remove_file(root.join("tool").join("fs.read.d").join("schema")).is_ok());
    assert!(symlink(
        outside.join("schema"),
        root.join("tool").join("fs.read.d").join("schema")
    )
    .is_ok());
    let report = inspect_object_layout(&root, ObjectClass::Tool, "fs.read");
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::NotControlFile(
            "tool/fs.read.d/schema".to_owned()
        )));
    assert!(!report
        .issues()
        .contains(&ObjectLayoutIssue::InvalidControlValue {
            path: "tool/fs.read.d/schema".to_owned(),
            value: "authority".to_owned()
        }));
}

#[test]
fn object_layout_rejects_symlink_class_directory_without_reading_target() {
    let root = clean_test_dir("object-layout-symlink-class");
    let outside = clean_test_dir("object-layout-symlink-class-outside");
    create_complete_object_layout(&outside, ObjectClass::Tool, "fs.read", "");
    write_text_file(
        &outside.join("tool").join("fs.read.d").join("schema"),
        "{\"authority\":\"root\"}\n",
    );
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(symlink(outside.join("tool"), root.join("tool")).is_ok());

    let report = inspect_object_layout(&root, ObjectClass::Tool, "fs.read");
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::MissingExecutable(
            "tool/fs.read".to_owned()
        )));
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::MissingControlDirectory(
            "tool/fs.read.d".to_owned()
        )));
    assert!(!report
        .issues()
        .contains(&ObjectLayoutIssue::InvalidControlValue {
            path: "tool/fs.read.d/schema".to_owned(),
            value: "authority".to_owned()
        }));
}
