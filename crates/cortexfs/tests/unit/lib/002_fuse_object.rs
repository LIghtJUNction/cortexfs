#[test]
#[expect(
    clippy::too_many_lines,
    reason = "single projection smoke test keeps related FUSE ABI assertions together"
)]
fn fuse_v1_projection_exposes_reference_tree_ops() {
    let root = reference_tree("fuse-v1-projection");
    write_fixture_file(&root.join("model").join("qwen"), 0o755);
    assert!(fs::create_dir_all(root.join("model").join("qwen.d")).is_ok());
    assert!(symlink("qwen", root.join("model").join("main")).is_ok());
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    let root_node = projection.root_node();
    let root_node = ok!(root_node);
    assert_eq!(root_node.inode(), FUSE_V1_ROOT_INODE);
    assert_eq!(root_node.abi_path(), "");
    assert_eq!(root_node.attr().file_type(), FuseV1FileType::Directory);

    let root_attr = projection.getattr_node(&root_node);
    assert!(matches!(
        root_attr,
        Ok(ref attr)
            if attr.abi_path().is_empty()
                && attr.file_type() == FuseV1FileType::Directory
    ));

    let entries = projection.readdir_node(&root_node);
    let entries = ok!(entries);
    let names = entries
        .iter()
        .map(super::FuseV1DirEntry::name)
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["agent", "bin", "home", "model", "shared", "status", "tool"]
    );

    let model_node = projection.lookup(&root_node, "model");
    assert!(matches!(
        model_node,
        Ok(ref node)
            if node.abi_path() == "model"
                && node.attr().file_type() == FuseV1FileType::Directory
    ));
    let Ok(model_node) = model_node else { return };
    let model_entries = projection.readdir("model");
    let model_entries = ok!(model_entries);
    let model_names = model_entries
        .iter()
        .map(super::FuseV1DirEntry::name)
        .collect::<Vec<_>>();
    assert_eq!(model_names, ["debug", "helper", "main"]);
    let main_node = projection.lookup(&model_node, "main");
    assert!(matches!(
        main_node,
        Ok(ref node)
            if node.abi_path() == "model/main"
                && node.attr().file_type() == FuseV1FileType::Symlink
    ));
    assert_eq!(
        projection.readlink("model/main"),
        Ok(PathBuf::from("/ctx/model/debug/echo"))
    );
    assert_eq!(
        projection.readlink("model/helper"),
        Ok(PathBuf::from("/ctx/model/debug/echo"))
    );
    let debug_node = projection.lookup(&model_node, "debug");
    assert!(matches!(
        debug_node,
        Ok(ref node)
            if node.abi_path() == "model/debug"
                && node.inode() != FUSE_V1_ROOT_INODE
                && node.attr().file_type() == FuseV1FileType::Directory
    ));
    let Ok(debug_node) = debug_node else { return };
    let echo_node = projection.lookup(&debug_node, "echo");
    assert!(matches!(
        echo_node,
        Ok(ref node)
            if node.abi_path() == "model/debug/echo"
                && node.inode() != FUSE_V1_ROOT_INODE
                && node.attr().file_type() == FuseV1FileType::Regular
    ));
    let echo_again = projection.node_for_path("model/debug/echo");
    assert!(
        matches!((echo_node, echo_again), (Ok(ref left), Ok(ref right)) if left.inode() == right.inode())
    );
    let echo_metadata = projection.read_to_string("model/debug/echo");
    assert!(matches!(
        echo_metadata,
        Ok(ref content)
            if content.starts_with(&format!("#!{CORTEXFS_OBJECT_RUNNER}\n"))
                && content.contains("# cortexfs.name=debug/echo\n")
    ));
    assert_eq!(
        projection.read_at("model/debug/echo", 0, 32),
        Ok(echo_metadata
            .unwrap_or_default()
            .bytes()
            .take(32)
            .collect::<Vec<_>>())
    );
    let echo_attr = projection.getattr("model/debug/echo");
    assert!(matches!(
        echo_attr,
        Ok(ref attr) if attr.mode() & 0o777 == 0o555
    ));
    let tool_metadata = projection.read_to_string("tool/fs.read");
    assert!(matches!(
        tool_metadata,
        Ok(ref content)
            if content.starts_with(&format!("#!{CORTEXFS_OBJECT_RUNNER}\n"))
                && content.contains("# cortexfs.object=tool\n")
                && content.contains("# cortexfs.name=fs.read\n")
                && !content.contains("#!/bin/sh")
    ));
    let agent_metadata = projection.read_to_string("agent/coder");
    assert!(matches!(
        agent_metadata,
        Ok(ref content)
            if content.starts_with(&format!("#!{CORTEXFS_OBJECT_RUNNER}\n"))
                && content.contains("# cortexfs.object=agent\n")
                && content.contains("# cortexfs.name=coder\n")
                && content.contains("# cortexfs.model=debug/echo\n")
                && !content.contains("reference-tree agent stub")
    ));
    let tool_attr = projection.getattr("tool/fs.read");
    assert!(matches!(
        tool_attr,
        Ok(ref attr)
            if attr.file_type() == FuseV1FileType::Regular && attr.mode() & 0o777 == 0o555
    ));
    let agent_attr = projection.getattr("agent/coder");
    assert!(matches!(
        agent_attr,
        Ok(ref attr)
            if attr.file_type() == FuseV1FileType::Regular && attr.mode() & 0o777 == 0o555
    ));
    assert_eq!(
        projection.lookup(&root_node, "../escape"),
        Err(FuseV1Error::InvalidPath)
    );
    assert_eq!(
        projection.lookup(&root_node, "missing"),
        Err(FuseV1Error::NotFound)
    );

    assert_eq!(
        projection.getattr("model/debug/echo.sock"),
        Err(FuseV1Error::NotFound)
    );
    let socket_attr = projection.getattr("agent/coder.sock");
    assert!(matches!(
        socket_attr,
        Ok(ref attr)
            if attr.file_type() == FuseV1FileType::Socket && attr.mode() & 0o777 == 0o777
    ));
    assert_eq!(
        projection.getattr("home/1000/tool/fs.read"),
        Err(FuseV1Error::NotFound)
    );
}

#[test]
fn fuse_v1_projection_reads_and_writes_control_files() {
    let root = reference_tree("fuse-v1-projection-control-files");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    assert_eq!(
        projection.read_to_string("status"),
        Ok("ready\n".to_owned())
    );
    assert_eq!(projection.read_at("status", 1, 3), Ok(b"ead".to_vec()));
    assert_eq!(projection.read_at("status", 128, 8), Ok(Vec::new()));
    assert!(projection
        .write_control_file("agent/coder.d/cwd", "/work/project\n")
        .is_ok());
    assert_eq!(
        projection.read_to_string("agent/coder.d/cwd"),
        Ok("/work/project\n".to_owned())
    );

    assert_eq!(
        projection.write_control_file("status", "busy\n"),
        Err(FuseV1Error::NotControlFile)
    );
    assert!(projection
        .write_control_file_at("agent/coder.d/status", 0, b"busy\n")
        .is_ok());
    assert_eq!(
        projection.read_to_string("agent/coder.d/status"),
        Ok("busy\n".to_owned())
    );
    assert_eq!(
        projection.write_control_file_at("agent/coder.d/status", 1, b"idle\n"),
        Err(FuseV1Error::InvalidOffset)
    );
    assert_eq!(
        projection.write_control_file_at("agent/coder.d/status", 0, &[0xff]),
        Err(FuseV1Error::InvalidContent)
    );
    assert_eq!(
        projection.write_control_file("../escape", "no\n"),
        Err(FuseV1Error::InvalidPath)
    );
    assert_eq!(
        projection.write_control_file(
            "agent/coder.d/cwd",
            &"x".repeat(MAX_FUSE_V1_SMALL_WRITE_BYTES + 1)
        ),
        Err(FuseV1Error::TooLarge)
    );
    assert_eq!(FuseV1Error::TooLarge.errno(), "EMSGSIZE");
    assert_eq!(FuseV1Error::InvalidOffset.errno(), "EINVAL");
}

#[test]
fn fuse_v1_projection_projects_configured_provider_models() {
    let root = reference_tree("fuse-v1-provider-model");
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("api.lmm.best.json"),
        r#"{
  "base_url": "https://api.lmm.best:9000/",
  "default_model": "gpt-5.4-mini",
  "enabled": true,
  "formats": ["openai.chat", "openai.responses"]
}
"#,
    );
    let projection = FuseV1Projection::new(&root).with_provider_config_dir(&providers);

    let model_entries = projection.readdir("model");
    assert!(model_entries.is_ok());
    let model_names = model_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(model_names, ["api.lmm.best", "debug", "helper", "main"]);

    let provider_entries = projection.readdir("model/api.lmm.best");
    assert!(provider_entries.is_ok());
    let provider_names = provider_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(provider_names, ["gpt-5.4-mini", "gpt-5.4-mini.d"]);

    let metadata = projection.read_to_string("model/api.lmm.best/gpt-5.4-mini");
    assert!(matches!(
        metadata,
        Ok(ref content)
            if content.starts_with(&format!("#!{CORTEXFS_OBJECT_RUNNER}\n"))
                && content.contains("# cortexfs.name=api.lmm.best/gpt-5.4-mini\n")
                && content.contains("# cortexfs.driver.default=openai-chat\n")
                && content.contains("# cortexfs.driver.agent=openai-responses,openai-chat\n")
    ));
    assert_eq!(
        projection.read_to_string("model/api.lmm.best/gpt-5.4-mini.d/driver"),
        Ok("default=openai-chat\nexec=openai-chat\nagent=openai-responses,openai-chat\n".to_owned())
    );
    assert_eq!(
        projection.read_to_string("model/api.lmm.best/gpt-5.4-mini.d/default"),
        Ok("base_url=https://api.lmm.best:9000/\n".to_owned())
    );
    let attr = projection.getattr("model/api.lmm.best/gpt-5.4-mini");
    assert!(matches!(attr, Ok(ref attr) if attr.mode() & 0o777 == 0o555));
}

#[test]
fn fuse_v1_projection_skips_disabled_provider_models() {
    let root = reference_tree("fuse-v1-disabled-provider-model");
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("api.lmm.best.json"),
        r#"{
  "base_url": "https://api.lmm.best:9000/",
  "default_model": "gpt-5.4-mini",
  "enabled": false,
  "formats": ["openai.chat"]
}
"#,
    );
    let projection = FuseV1Projection::new(&root).with_provider_config_dir(&providers);

    let model_entries = projection.readdir("model");
    assert!(model_entries.is_ok());
    let model_names = model_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(model_names, ["debug", "helper", "main"]);
    assert_eq!(
        projection.getattr("model/api.lmm.best"),
        Err(FuseV1Error::NotFound)
    );
}

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
    assert_eq!(
        ensure_v1_reference_tree(&root),
        Err(ReferenceTreeError::CannotSocket)
    );
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
