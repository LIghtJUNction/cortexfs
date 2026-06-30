#[test]
fn fuse_v1_projection_root_is_traversable_when_backing_root_is_private() {
    let root = reference_tree("fuse-v1-private-backing-root");
    assert!(fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).is_ok());
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    let attr = projection.getattr("");
    assert!(matches!(
        attr,
        Ok(ref attr)
            if attr.file_type() == FuseV1FileType::Directory
                && attr.mode() & 0o777 == 0o755
    ));

    let status_attr = projection.getattr("status");
    assert!(matches!(status_attr, Ok(ref attr) if attr.mode() & 0o777 == 0o644));
}

#[test]
fn fuse_v1_paths_reject_control_characters() {
    let root = Path::new("/ctx");

    assert_eq!(
        resolve_fuse_abi_path(root, "agent/coder\u{1b}.d/status"),
        Err(FuseV1Error::InvalidPath)
    );
    assert_eq!(
        resolve_fuse_abi_path(root, "agent/coder\r.d/status"),
        Err(FuseV1Error::InvalidPath)
    );
}

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
    assert_eq!(model_names, ["debug", "helper", "main", "route"]);
    let main_node = projection.lookup(&model_node, "main");
    assert!(matches!(
        main_node,
        Ok(ref node)
            if node.abi_path() == "model/main"
                && node.attr().file_type() == FuseV1FileType::Symlink
    ));
    assert_eq!(
        projection.readlink("model/main"),
        Ok(PathBuf::from("/ctx/model/openai/gpt-5.5"))
    );
    assert_eq!(
        projection.readlink("model/helper"),
        Ok(PathBuf::from("/ctx/model/openai/codex-auto-review"))
    );
    assert!(projection
        .set_model_alias("model/main", Path::new("api.lmm.best/gpt-5.4"))
        .is_ok());
    assert_eq!(
        projection.readlink("model/main"),
        Ok(PathBuf::from("/ctx/model/api.lmm.best/gpt-5.4"))
    );
    assert_eq!(
        projection.set_model_alias("model/test", Path::new("api.lmm.best/gpt-5.4")),
        Err(FuseV1Error::NotControlFile)
    );
    assert_eq!(
        projection.set_model_alias("model/main", Path::new("main")),
        Err(FuseV1Error::InvalidPath)
    );
    assert!(projection.remove_model_alias("model/main").is_ok());
    assert_eq!(
        projection.readlink("model/main"),
        Ok(PathBuf::from("/ctx/model/openai/gpt-5.5"))
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
    let debug_entries = projection.readdir("model/debug");
    let debug_entries = ok!(debug_entries);
    let debug_names = debug_entries
        .iter()
        .map(super::FuseV1DirEntry::name)
        .collect::<Vec<_>>();
    assert_eq!(debug_names, ["echo", "echo.d"]);
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
    let tool_metadata = projection.read_to_string("tool/tsh");
    assert!(matches!(
        tool_metadata,
        Ok(ref content)
            if content.starts_with(&format!("#!{CORTEXFS_OBJECT_RUNNER}\n"))
                && content.contains("# cortexfs.object=tool\n")
                && content.contains("# cortexfs.name=tsh\n")
                && !content.contains("#!/bin/sh")
    ));
    let agent_metadata = projection.read_to_string("agent/coder");
    assert!(matches!(
        agent_metadata,
        Ok(ref content)
            if content.starts_with(&format!("#!{CORTEXFS_OBJECT_RUNNER}\n"))
                && content.contains("# cortexfs.object=agent\n")
                && content.contains("# cortexfs.name=coder\n")
                && content.contains("# cortexfs.model=main\n")
                && !content.contains("reference-tree agent stub")
    ));
    let tool_attr = projection.getattr("tool/tsh");
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
fn fuse_v1_projection_model_alias_does_not_reuse_predictable_temp_symlink() {
    let root = reference_tree("fuse-v1-model-alias-temp");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let old_predictable_temp = root
        .join("model")
        .join(format!(".main.tmp.{}", std::process::id()));
    assert!(symlink("/ctx/model/keep", &old_predictable_temp).is_ok());

    assert!(projection
        .set_model_alias("model/main", Path::new("api.lmm.best/gpt-5.4"))
        .is_ok());

    assert!(matches!(
        fs::read_link(&old_predictable_temp),
        Ok(ref target) if target == Path::new("/ctx/model/keep")
    ));
    assert_eq!(
        projection.readlink("model/main"),
        Ok(PathBuf::from("/ctx/model/api.lmm.best/gpt-5.4"))
    );
    let temp_leftovers = fs::read_dir(root.join("model"))
        .map_or(usize::MAX, |entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().starts_with(".main.tmp-"))
                .count()
        });
    assert_eq!(temp_leftovers, 0);
}

#[test]
fn fuse_v1_projection_renames_model_alias_symlink_atomically() {
    let root = reference_tree("fuse-v1-model-alias-rename");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    assert!(projection
        .set_model_alias_symlink("model/tmp", Path::new("api.lmm.best/gpt-5.4"))
        .is_ok());
    assert!(projection
        .rename_model_alias_symlink("model/tmp", "model/main")
        .is_ok());

    assert_eq!(
        projection.readlink("model/main"),
        Ok(PathBuf::from("/ctx/model/api.lmm.best/gpt-5.4"))
    );
    assert!(!root.join("model").join("tmp").exists());
}

#[test]
fn fuse_v1_projection_model_alias_rejects_symlink_model_directory_without_touching_target() {
    let root = clean_test_dir("fuse-v1-model-alias-symlink-model");
    let outside = clean_test_dir("fuse-v1-model-alias-symlink-model-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(symlink(&outside, root.join("model")).is_ok());
    assert!(symlink("/ctx/model/keep", outside.join("main")).is_ok());
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    assert_eq!(
        projection.readlink("model/main"),
        Ok(PathBuf::from("/ctx/model/openai/gpt-5.5"))
    );
    assert_eq!(
        projection.set_model_alias("model/main", Path::new("api.lmm.best/gpt-5.4")),
        Err(FuseV1Error::Io)
    );
    assert_eq!(projection.remove_model_alias("model/main"), Err(FuseV1Error::Io));
    assert!(matches!(
        fs::read_link(outside.join("main")),
        Ok(ref target) if target == Path::new("/ctx/model/keep")
    ));
    let temp_leftovers = fs::read_dir(&outside).map_or(usize::MAX, |entries| {
        entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".main.tmp-"))
            .count()
    });
    assert_eq!(temp_leftovers, 0);
}

#[test]
fn fuse_v1_projection_read_at_uses_linux_file_read_shape() {
    let root = reference_tree("fuse-v1-read-at");
    assert!(fs::write(root.join("shared").join("readable"), b"abcdef").is_ok());
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    assert_eq!(
        projection.read_at("shared/readable", 2, 3),
        Ok(b"cde".to_vec())
    );
    assert_eq!(projection.read_at("shared/readable", 32, 8), Ok(Vec::new()));
    assert_eq!(projection.read_at("shared", 0, 8), Err(FuseV1Error::NotFile));
}

#[test]
fn fuse_v1_projection_write_control_file_at_enforces_whole_text_control_writes() {
    let root = reference_tree("fuse-v1-write-control-at");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    assert_eq!(
        projection.write_control_file_at("agent/coder.d/label", 1, b"worker"),
        Err(FuseV1Error::InvalidOffset)
    );
    assert_eq!(
        projection.write_control_file_at("agent/coder.d/label", 0, &[0xff]),
        Err(FuseV1Error::InvalidContent)
    );
    assert_eq!(
        projection.write_control_file_at("status", 0, b"worker"),
        Err(FuseV1Error::NotControlFile)
    );
    assert!(projection
        .write_control_file_at("agent/coder.d/label", 0, b"worker")
        .is_ok());
    assert!(matches!(
        projection.read_to_string("agent/coder.d/label"),
        Ok(ref content) if content == "worker"
    ));
}
