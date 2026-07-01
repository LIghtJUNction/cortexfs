mod model_alias_tests {
    use std::collections::{HashMap, HashSet};
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::sync::Mutex;

    use cortexfs::{ensure_v1_reference_tree, FuseV1Error, FuseV1Projection};
    use fuser::{Filesystem, INodeNo};

    use super::super::{CortexFuse, FUSE_V1_ROOT_INODE};

    #[test]
    fn unlink_model_path_ignores_non_symlink_provider_entries() {
        let root = super::unique_mount_test_dir("model-alias-provider-dir");
        assert!(ensure_v1_reference_tree(&root).is_ok());
        let provider = root.join("model").join("fixture");
        assert!(fs::create_dir_all(&provider).is_ok());
        let fs = mount_with_model_inode(&root);

        assert_eq!(fs.unlink_model_path(INodeNo(42), OsStr::new("fixture")), Ok(false));
        assert!(provider.is_dir());
    }

    #[test]
    fn unlink_model_path_removes_model_alias_symlinks() {
        let root = super::unique_mount_test_dir("model-alias-symlink");
        assert!(ensure_v1_reference_tree(&root).is_ok());
        let alias = root.join("model").join("scratch");
        assert!(symlink("/ctx/model/debug/echo", &alias).is_ok());
        let fs = mount_with_model_inode(&root);

        assert_eq!(fs.unlink_model_path(INodeNo(42), OsStr::new("scratch")), Ok(true));
        assert!(!alias.exists());
    }

    #[test]
    fn forget_inode_drops_path_after_last_lookup_reference() -> Result<(), FuseV1Error> {
        let root = super::unique_mount_test_dir("forget-inode-lookup-count");
        assert!(ensure_v1_reference_tree(&root).is_ok());
        let fs = mount_with_model_inode(&root);
        let node = fs.projection.node_for_path("status")?;
        let inode = INodeNo(node.inode());

        assert_eq!(fs.remember_lookup(&node), Ok(()));
        assert_eq!(fs.remember_lookup(&node), Ok(()));
        assert_eq!(fs.path_for_inode(inode), Ok("status".to_owned()));
        assert_eq!(fs.forget_inode(inode, 1), Ok(()));
        assert_eq!(fs.path_for_inode(inode), Ok("status".to_owned()));
        assert_eq!(fs.forget_inode(inode, 1), Ok(()));
        assert_eq!(fs.path_for_inode(inode), Err(FuseV1Error::NotFound));
        Ok(())
    }

    #[test]
    fn forget_inode_preserves_path_without_lookup_reference() -> Result<(), FuseV1Error> {
        let root = super::unique_mount_test_dir("forget-inode-without-lookup-count");
        assert!(ensure_v1_reference_tree(&root).is_ok());
        let fs = mount_with_model_inode(&root);
        let node = fs.projection.node_for_path("status")?;
        let inode = INodeNo(node.inode());

        assert_eq!(fs.remember(&node), Ok(()));
        assert_eq!(fs.forget_inode(inode, 1), Ok(()));
        assert_eq!(fs.path_for_inode(inode), Ok("status".to_owned()));
        Ok(())
    }

    #[test]
    fn destroy_drops_fuse_lifecycle_caches() -> Result<(), FuseV1Error> {
        let root = super::unique_mount_test_dir("destroy-fuse-caches");
        assert!(ensure_v1_reference_tree(&root).is_ok());
        let mut fs = mount_with_model_inode(&root);
        let node = fs.projection.node_for_path("status")?;
        let inode = INodeNo(node.inode());

        assert_eq!(fs.remember_lookup(&node), Ok(()));
        assert_eq!(fs.path_for_inode(inode), Ok("status".to_owned()));
        assert!(fs
            .socket_overlays
            .lock()
            .is_ok_and(|mut sockets| sockets.insert("agent/coder.sock".to_owned())));
        fs.destroy();

        assert_eq!(fs.path_for_inode(inode), Err(FuseV1Error::NotFound));
        assert_eq!(fs.path_for_inode(INodeNo(FUSE_V1_ROOT_INODE)), Err(FuseV1Error::NotFound));
        assert!(fs.lookup_counts.lock().is_ok_and(|counts| counts.is_empty()));
        assert!(fs.socket_overlays.lock().is_ok_and(|sockets| sockets.is_empty()));
        Ok(())
    }

    fn mount_with_model_inode(root: &std::path::Path) -> CortexFuse {
        CortexFuse {
            projection: FuseV1Projection::new(root.to_path_buf()),
            paths: Mutex::new(HashMap::from([
                (FUSE_V1_ROOT_INODE, String::new()),
                (42, "model".to_owned()),
            ])),
            lookup_counts: Mutex::new(HashMap::new()),
            socket_overlays: Mutex::new(HashSet::new()),
        }
    }
}
