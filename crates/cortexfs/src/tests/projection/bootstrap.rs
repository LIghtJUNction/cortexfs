#[test]
fn reference_tree_bootstrap_rejects_conflicting_symlink_and_socket_paths() {
    let root = clean_test_dir("reference-tree-conflict");
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
    assert!(
        symlink(
            root.join("missing-runtime.sock"),
            root.join("agent").join("coder.sock")
        )
        .is_ok()
    );

    assert!(ensure_v1_reference_tree(&root).is_ok());
    let metadata = fs::symlink_metadata(root.join("agent").join("coder.sock"));
    assert!(matches!(metadata, Ok(ref metadata) if metadata.file_type().is_socket()));
}

#[test]
fn reference_tree_bootstrap_repairs_plain_socket_placeholder_owner() {
    if !nix::unistd::Uid::effective().is_root() {
        return;
    }
    let root = clean_test_dir("reference-tree-socket-owner-upgrade");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let socket = root.join("agent/coder.sock");
    assert!(
        nix::unistd::fchownat(
            nix::fcntl::AT_FDCWD,
            &socket,
            Some(nix::unistd::Uid::from_raw(0)),
            Some(nix::unistd::Gid::from_raw(0)),
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .is_ok()
    );

    assert!(ensure_v1_reference_tree(&root).is_ok());
    assert!(matches!(
        fs::symlink_metadata(socket),
        Ok(metadata) if metadata.uid() == 1000 && metadata.gid() == 1000
    ));
}

#[test]
fn reference_socket_owner_repair_requires_effective_root() {
    assert!(should_repair_reference_owner(0));
    assert!(!should_repair_reference_owner(1000));
    assert!(!should_repair_reference_owner(u32::MAX));
}

#[test]
fn reference_home_chown_rejects_symlinked_ancestor() {
    let root = clean_test_dir("reference-home-chown-ancestor");
    let external = clean_test_dir("reference-home-chown-external");
    let victim = external.join("victim");
    assert!(fs::create_dir_all(&*root).is_ok());
    assert!(fs::create_dir_all(&*external).is_ok());
    assert!(fs::write(&victim, "outside\n").is_ok());
    assert!(symlink(&*external, root.join("ancestor")).is_ok());
    let before = ok!(fs::metadata(&victim));
    let uid = nix::unistd::Uid::effective().as_raw();
    let gid = nix::unistd::Gid::effective().as_raw();

    let result = super::chown_reference_home_entry(&root.join("ancestor/victim"), uid, gid);

    let after = ok!(fs::metadata(&victim));
    assert!(result.is_err());
    assert_eq!(
        (
            after.dev(),
            after.ino(),
            after.uid(),
            after.gid(),
            after.ctime(),
            after.ctime_nsec(),
        ),
        (
            before.dev(),
            before.ino(),
            before.uid(),
            before.gid(),
            before.ctime(),
            before.ctime_nsec(),
        )
    );
}

#[test]
fn reference_home_chown_changes_child_symlink_not_target() {
    let root = clean_test_dir("reference-home-chown-child-link");
    let external = clean_test_dir("reference-home-chown-child-target");
    let directory = root.join("home");
    let target = external.join("target");
    let link = directory.join("child");
    assert!(fs::create_dir_all(&directory).is_ok());
    assert!(fs::create_dir_all(&*external).is_ok());
    assert!(fs::write(&target, "outside\n").is_ok());
    assert!(symlink(&target, &link).is_ok());
    let target_before = ok!(fs::metadata(&target));
    let link_before = ok!(fs::symlink_metadata(&link));
    let root_user = nix::unistd::Uid::effective().is_root();
    let uid = if root_user {
        target_before.uid().saturating_add(1)
    } else {
        nix::unistd::Uid::effective().as_raw()
    };
    let gid = if root_user {
        target_before.gid().saturating_add(1)
    } else {
        nix::unistd::Gid::effective().as_raw()
    };

    let result = super::chown_reference_home_entry(&directory, uid, gid);

    let target_after = ok!(fs::metadata(&target));
    let link_after = ok!(fs::symlink_metadata(&link));
    assert!(result.is_ok());
    assert_eq!((link_after.uid(), link_after.gid()), (uid, gid));
    assert_eq!(link_after.ino(), link_before.ino());
    assert_ne!(
        (link_after.ctime(), link_after.ctime_nsec()),
        (link_before.ctime(), link_before.ctime_nsec())
    );
    assert_eq!(
        (
            target_after.dev(),
            target_after.ino(),
            target_after.uid(),
            target_after.gid(),
            target_after.ctime(),
            target_after.ctime_nsec(),
        ),
        (
            target_before.dev(),
            target_before.ino(),
            target_before.uid(),
            target_before.gid(),
            target_before.ctime(),
            target_before.ctime_nsec(),
        )
    );
}

#[test]
fn reference_home_chown_repairs_plain_file_and_directory() {
    let root = clean_test_dir("reference-home-chown-plain");
    let directory = root.join("home");
    let file = directory.join("entry");
    assert!(fs::create_dir_all(&directory).is_ok());
    assert!(fs::write(&file, "plain\n").is_ok());
    let directory_before = ok!(fs::metadata(&directory));
    let file_before = ok!(fs::metadata(&file));
    let uid = nix::unistd::Uid::effective().as_raw();
    let gid = nix::unistd::Gid::effective().as_raw();

    assert!(super::chown_reference_home_entry(&directory, uid, gid).is_ok());
    assert!(super::chown_reference_home_entry(&file, uid, gid).is_ok());

    let directory_after = ok!(fs::metadata(&directory));
    let file_after = ok!(fs::metadata(&file));
    assert_eq!(directory_after.ino(), directory_before.ino());
    assert_eq!(file_after.ino(), file_before.ino());
    assert_eq!((directory_after.uid(), directory_after.gid()), (uid, gid));
    assert_eq!((file_after.uid(), file_after.gid()), (uid, gid));
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
fn fuse_projection_exposes_object_hook_directories() {
    let root = clean_test_dir("fuse-projection-object-hooks");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let projection = FuseV1Projection::new(root.as_path());

    let model_entries = ok!(projection.readdir("model/debug/echo.d"));
    assert!(
        !model_entries
            .iter()
            .any(|entry| entry.name() == OBJECT_HOOK_DIR)
    );

    for path in ["agent/coder.d", "tool/fs.read.d"] {
        let entries = ok!(projection.readdir(path));
        assert!(entries.iter().any(|entry| entry.name() == OBJECT_HOOK_DIR));

        let hook_entries = ok!(projection.readdir(&format!("{path}/{OBJECT_HOOK_DIR}")));
        for phase in OBJECT_HOOK_PHASE_DIRS {
            assert!(hook_entries.iter().any(|entry| entry.name() == *phase));
            let phase_entries =
                ok!(projection.readdir(&format!("{path}/{OBJECT_HOOK_DIR}/{phase}")));
            assert!(phase_entries.is_empty());
        }
    }
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

    assert_eq!(
        model.executable(),
        root.join("model").join("debug").join("echo")
    );
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
use super::*;
