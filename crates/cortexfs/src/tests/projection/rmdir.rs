#[test]
fn fuse_projection_removes_empty_home_plain_dir() {
    let root = reference_tree("fuse-rmdir-empty-home-dir");
    let projection =
        FuseProjection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let path = "home/1000/agent/executor/cache/delete-me";

    assert!(fs::create_dir_all(root.join(path)).is_ok());
    assert_eq!(projection.remove_empty_plain_dir(path), Ok(()));
    assert!(!root.join(path).exists());
}

#[test]
fn fuse_projection_rmdir_rejects_non_empty_dir() {
    let root = reference_tree("fuse-rmdir-non-empty");
    let projection =
        FuseProjection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let path = "home/1000/agent/executor/cache/non-empty";

    assert!(fs::create_dir_all(root.join(path)).is_ok());
    assert!(fs::write(root.join(path).join("kept"), "data\n").is_ok());

    assert_eq!(
        projection.remove_empty_plain_dir(path),
        Err(FuseError::NotEmpty)
    );
    assert!(root.join(path).is_dir());
}

#[test]
fn fuse_projection_rmdir_rejects_root_and_global_abi_dirs() {
    let root = reference_tree("fuse-rmdir-abi-dirs");
    let projection =
        FuseProjection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    assert_eq!(
        projection.remove_empty_plain_dir(""),
        Err(FuseError::ReadOnly)
    );
    assert_eq!(
        projection.remove_empty_plain_dir("model"),
        Err(FuseError::ReadOnly)
    );
    assert_eq!(
        projection.remove_empty_plain_dir("agent/executor.d"),
        Err(FuseError::ReadOnly)
    );
}

#[test]
fn fuse_projection_rmdir_rejects_symlink_dir_path() {
    let root = reference_tree("fuse-rmdir-symlink");
    let projection =
        FuseProjection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let target = root.join("home/1000/agent/executor/cache/target");
    let link = root.join("home/1000/agent/executor/cache/link-dir");

    assert!(fs::create_dir_all(&target).is_ok());
    assert!(symlink("target", &link).is_ok());

    assert_eq!(
        projection.remove_empty_plain_dir("home/1000/agent/executor/cache/link-dir"),
        Err(FuseError::NotDirectory)
    );
    assert!(link.is_symlink());
}

#[test]
fn fuse_projection_rolls_back_owned_agent_lifecycle_paths_only() {
    let root = reference_tree("fuse-rmdir-agent-lifecycle");
    let projection =
        FuseProjection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let uid = nix::unistd::Uid::current().as_raw();
    let gid = nix::unistd::Gid::current().as_raw();

    assert_eq!(
        projection.create_layout_dir("agent/scratch.d", uid, gid, 0o755),
        Ok(())
    );
    assert_eq!(
        projection.create_layout_file("agent/scratch.d/.owner.tmp-1-1-0", uid, gid, 0o600),
        Ok(())
    );
    assert_eq!(
        projection.remove_layout_file("agent/scratch.d/.owner.tmp-1-1-0", uid),
        Ok(())
    );
    assert_eq!(
        projection.remove_empty_layout_dir("agent/scratch.d", uid),
        Ok(())
    );
    assert!(!root.join("agent/scratch.d").exists());

    let foreign = uid.saturating_add(1);
    assert_eq!(
        projection.remove_layout_file("agent/executor", foreign),
        Err(FuseError::PermissionDenied)
    );
    assert!(root.join("agent/executor").exists());
    assert_eq!(
        projection.remove_empty_layout_dir("agent", uid),
        Err(FuseError::NotControlFile)
    );
    assert_eq!(
        projection.remove_layout_file("home/1000/.profile", uid),
        Err(FuseError::NotControlFile)
    );
    assert!(fs::write(root.join("agent/executor.d/owner"), format!("{uid}\n")).is_ok());
    let target = format!("/run/user/{uid}/cortexfs/agent/test/executor.sock");
    assert!(fs::remove_file(root.join("agent/executor.sock")).is_ok());
    assert!(symlink(&target, root.join("agent/executor.sock")).is_ok());

    assert_eq!(
        projection.remove_agent_definition("agent/executor", uid),
        Err(FuseError::Busy)
    );
    assert_eq!(
        projection.remove_empty_layout_dir("agent/executor.d", uid),
        Err(FuseError::ReadOnly)
    );
    assert!(root.join("agent/executor").is_file());
    assert!(root.join("agent/executor.d").is_dir());
}
use super::*;
