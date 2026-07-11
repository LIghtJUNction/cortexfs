#[test]
fn fuse_v1_projection_removes_empty_home_plain_dir() {
    let root = reference_tree("fuse-v1-rmdir-empty-home-dir");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let path = "home/1000/agent/coder/cache/delete-me";

    assert!(fs::create_dir_all(root.join(path)).is_ok());
    assert_eq!(projection.remove_empty_plain_dir(path), Ok(()));
    assert!(!root.join(path).exists());
}

#[test]
fn fuse_v1_projection_rmdir_rejects_non_empty_dir() {
    let root = reference_tree("fuse-v1-rmdir-non-empty");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let path = "home/1000/agent/coder/cache/non-empty";

    assert!(fs::create_dir_all(root.join(path)).is_ok());
    assert!(fs::write(root.join(path).join("kept"), "data\n").is_ok());

    assert_eq!(
        projection.remove_empty_plain_dir(path),
        Err(FuseV1Error::NotEmpty)
    );
    assert!(root.join(path).is_dir());
}

#[test]
fn fuse_v1_projection_rmdir_rejects_root_and_global_abi_dirs() {
    let root = reference_tree("fuse-v1-rmdir-abi-dirs");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    assert_eq!(
        projection.remove_empty_plain_dir(""),
        Err(FuseV1Error::ReadOnly)
    );
    assert_eq!(
        projection.remove_empty_plain_dir("model"),
        Err(FuseV1Error::ReadOnly)
    );
    assert_eq!(
        projection.remove_empty_plain_dir("agent/coder.d"),
        Err(FuseV1Error::ReadOnly)
    );
}

#[test]
fn fuse_v1_projection_rmdir_rejects_symlink_dir_path() {
    let root = reference_tree("fuse-v1-rmdir-symlink");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let target = root.join("home/1000/agent/coder/cache/target");
    let link = root.join("home/1000/agent/coder/cache/link-dir");

    assert!(fs::create_dir_all(&target).is_ok());
    assert!(symlink("target", &link).is_ok());

    assert_eq!(
        projection.remove_empty_plain_dir("home/1000/agent/coder/cache/link-dir"),
        Err(FuseV1Error::NotDirectory)
    );
    assert!(link.is_symlink());
}

#[test]
fn fuse_v1_projection_rolls_back_owned_agent_lifecycle_paths_only() {
    let root = reference_tree("fuse-v1-rmdir-agent-lifecycle");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
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
        projection.remove_layout_file("agent/coder", foreign),
        Err(FuseV1Error::PermissionDenied)
    );
    assert!(root.join("agent/coder").exists());
    assert_eq!(
        projection.remove_empty_layout_dir("agent", uid),
        Err(FuseV1Error::NotControlFile)
    );
    assert_eq!(
        projection.remove_layout_file("home/1000/.profile", uid),
        Err(FuseV1Error::NotControlFile)
    );
}
use super::*;
