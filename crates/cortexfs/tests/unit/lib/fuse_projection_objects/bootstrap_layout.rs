#[test]
fn reference_tree_bootstrap_rejects_conflicting_symlink_and_socket_paths() {
    let root = clean_test_dir("reference-tree-conflict");
    write_text_file(&root.join("home").join("1000").join("model").join("coder"), "not link\n");
    assert_eq!(
        ensure_v1_reference_tree(&root),
        Err(ReferenceTreeError::CannotLink)
    );

    assert!(fs::remove_dir_all(&root).is_ok());
    write_text_file(&root.join("agent").join("coder.sock"), "not socket\n");
    assert!(matches!(
        ensure_v1_reference_tree(&root),
        Err(ReferenceTreeError::CannotSocket(_))
    ));
}

#[test]
fn reference_tree_bootstrap_replaces_stale_socket_symlink() {
    let root = clean_test_dir("reference-tree-stale-socket-symlink");
    assert!(fs::create_dir_all(root.join("agent")).is_ok());
    assert!(symlink(
        root.join("missing-runtime.sock"),
        root.join("agent").join("coder.sock")
    )
    .is_ok());

    assert!(ensure_v1_reference_tree(&root).is_ok());
    let metadata = fs::symlink_metadata(root.join("agent").join("coder.sock"));
    assert!(matches!(metadata, Ok(ref metadata) if metadata.file_type().is_socket()));
}

#[test]
fn object_layout_accepts_model_agent_and_tool_triples() {
    let root = clean_test_dir("object-layout-ok");
    create_complete_object_layout(&root, ObjectClass::Model, "debug/echo", "socket");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "");
    create_complete_object_layout(&root, ObjectClass::Tool, "fs.read", "");
    let _model_socket = bind_socket(&root.join("model").join("debug").join("echo.sock"));
    let _agent_socket = bind_socket(&root.join("agent").join("coder.sock"));

    let model = inspect_object_layout(&root, ObjectClass::Model, "debug/echo");
    let agent = inspect_object_layout(&root, ObjectClass::Agent, "coder");
    let tool = inspect_object_layout(&root, ObjectClass::Tool, "fs.read");
    assert!(model.is_ok());
    assert!(agent.is_ok());
    assert!(tool.is_ok());
}

#[test]
fn executable_object_bootstrap_installs_model_and_tool_wrappers() {
    let root = clean_test_dir("object-bootstrap");
    let target = root.join("runtime").join("echo-jsonl");

    write_fixture_file(&target, 0o755);

    let model = install_executable_object_wrapper(
        &root,
        ObjectClass::Model,
        "debug/echo",
        &target.display().to_string(),
        &[
            ("cap", "chat\nstream\ntool_call_syntax"),
            ("session", "none"),
            ("id", "debug/echo"),
        ],
    );
    let model = ok!(model);
    let tool = install_executable_object_wrapper(
        &root,
        ObjectClass::Tool,
        "fs.read",
        &target.display().to_string(),
        &[
            ("description", "Read a visible file"),
            ("schema", "{\"type\":\"object\",\"properties\":{}}"),
            ("policy", "allow coder_t tool:fs.read execute"),
        ],
    );
    let tool = ok!(tool);

    assert_eq!(model.executable(), root.join("model").join("debug").join("echo"));
    assert_eq!(tool.control_dir(), root.join("tool").join("fs.read.d"));
    assert!(inspect_object_layout(&root, ObjectClass::Model, "debug/echo").is_ok());
    assert!(inspect_object_layout(&root, ObjectClass::Tool, "fs.read").is_ok());

    let wrapper = fs::read_to_string(root.join("tool").join("fs.read"));
    let wrapper = ok!(wrapper);
    assert!(wrapper.starts_with("#!/bin/sh\n"));
    assert!(wrapper.contains("exec '"));
    let permissions = fs::metadata(root.join("tool").join("fs.read"))
        .map(|metadata| metadata.permissions().mode());
    let permissions = ok!(permissions);
    assert_ne!(permissions & 0o111, 0);
}
