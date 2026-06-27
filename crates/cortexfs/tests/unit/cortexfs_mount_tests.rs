use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixListener;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use cortexfs::{ensure_v1_reference_tree, FuseV1Attr, FuseV1FileType, FUSE_V1_ROOT_INODE};
use fuser::{FileType, INodeNo};

use super::{
    child_path, estimate_tokens_from_bytes, file_attr, mount_statfs_for_source, parent_inode,
    remove_backing_socket_entry, sanitize_mount_statfs, CortexFuse, MountStatfs,
};

#[test]
fn file_attr_maps_projection_attributes_to_fuser_attributes() {
    let attr = FuseV1Attr::new(
        "tool/fs.read".to_owned(),
        FuseV1FileType::Regular,
        1025,
        0o644,
    );
    let mapped = file_attr(77, &attr);

    assert_eq!(mapped.ino, INodeNo(77));
    assert_eq!(mapped.size, 1025);
    assert_eq!(mapped.blocks, 3);
    assert_eq!(mapped.kind, FileType::RegularFile);
    assert_eq!(mapped.perm, 0o644);
    assert_eq!(mapped.nlink, 1);
}

#[test]
fn parent_inode_uses_known_parent_or_root() {
    let paths = Mutex::new(HashMap::from([
        (FUSE_V1_ROOT_INODE, String::new()),
        (42, "agent/coder.d".to_owned()),
    ]));

    assert_eq!(parent_inode("agent/coder.d/status", &paths), Ok(42));
    assert_eq!(parent_inode("agent", &paths), Ok(FUSE_V1_ROOT_INODE));
    assert_eq!(parent_inode("", &paths), Ok(FUSE_V1_ROOT_INODE));
}

#[test]
fn child_path_rejects_special_or_escaped_names() {
    assert_eq!(child_path("", "agent"), Some("agent".to_owned()));
    assert_eq!(
        child_path("agent", "coder.sock"),
        Some("agent/coder.sock".to_owned())
    );
    assert_eq!(child_path("", ""), None);
    assert_eq!(child_path("", "."), None);
    assert_eq!(child_path("", ".."), None);
    assert_eq!(child_path("agent", "../coder.sock"), None);
    assert_eq!(child_path("agent", "coder/sock"), None);
    assert_eq!(child_path("agent", "coder\0sock"), None);
}

#[test]
fn remove_backing_socket_entry_refuses_plain_files() {
    let root = unique_mount_test_dir("socket-unlink-plain-file");
    assert!(fs::create_dir_all(root.join("agent")).is_ok());
    let path = root.join("agent").join("coder.sock");
    assert!(fs::write(&path, "not a socket").is_ok());

    let removed = remove_backing_socket_entry(&root, "agent/coder.sock");

    assert!(removed.is_err());
    assert!(path.exists());
}

#[test]
fn remove_backing_socket_entry_allows_socket_inode_and_symlink_entry() {
    let root = unique_mount_test_dir("socket-unlink-allowed");
    assert!(fs::create_dir_all(root.join("agent")).is_ok());
    let socket = root.join("agent").join("coder.sock");
    let listener = UnixListener::bind(&socket);
    assert!(listener.is_ok());

    assert!(remove_backing_socket_entry(&root, "agent/coder.sock").is_ok());
    assert!(!socket.exists());

    let outside = root.join("runtime.sock");
    let outside_listener = UnixListener::bind(&outside);
    assert!(outside_listener.is_ok());
    let link = root.join("agent").join("reviewer.sock");
    assert!(symlink(&outside, &link).is_ok());

    assert!(remove_backing_socket_entry(&root, "agent/reviewer.sock").is_ok());
    assert!(!link.exists());
    assert!(outside.exists());
}

#[test]
fn remove_backing_socket_entry_rejects_symlink_parent_without_removing_target() {
    let root = unique_mount_test_dir("socket-unlink-symlink-parent");
    let outside = unique_mount_test_dir("socket-unlink-symlink-parent-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(symlink(&outside, root.join("agent")).is_ok());
    let socket = outside.join("coder.sock");
    let listener = UnixListener::bind(&socket);
    assert!(listener.is_ok());

    let removed = remove_backing_socket_entry(&root, "agent/coder.sock");

    assert!(removed.is_err());
    assert!(socket.exists());
}

#[test]
fn unlink_model_path_rejects_symlink_model_parent_without_removing_target() {
    let root = unique_mount_test_dir("model-unlink-symlink-parent");
    let outside = unique_mount_test_dir("model-unlink-symlink-parent-outside");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    assert!(fs::remove_dir_all(root.join("model")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(fs::write(outside.join("temp"), "keep\n").is_ok());
    assert!(symlink(&outside, root.join("model")).is_ok());
    let fs = CortexFuse {
        projection: cortexfs::FuseV1Projection::new(root.to_path_buf()),
        paths: Mutex::new(HashMap::from([(42, "model".to_owned())])),
        socket_overlays: Mutex::new(HashSet::new()),
    };

    assert_eq!(
        fs.unlink_model_path(INodeNo(42), std::ffi::OsStr::new("temp")),
        Err(cortexfs::FuseV1Error::Io)
    );
    assert_eq!(
        fs::read_to_string(outside.join("temp")).unwrap_or_default(),
        "keep\n"
    );
}

#[test]
fn statfs_sanitizes_zero_totals_to_avoid_false_full_mount() {
    let stats = sanitize_mount_statfs(MountStatfs {
        blocks: 0,
        blocks_free: 0,
        blocks_available: 0,
        files: 0,
        files_free: 0,
        block_size: 0,
        name_max: 0,
        fragment_size: 0,
    });

    assert_eq!(stats.blocks, 1024 * 1024);
    assert_eq!(stats.blocks_free, (1024 * 1024) - 1024);
    assert_eq!(stats.blocks_available, (1024 * 1024) - 1024);
    assert_eq!(stats.files, 1024 * 1024);
    assert_eq!(stats.files_free, (1024 * 1024) - 1024);
    assert!(stats.files_free > stats.files * 99 / 100);
    assert_eq!(stats.block_size, 1);
    assert_eq!(stats.name_max, 1);
    assert_eq!(stats.fragment_size, 1);
}

#[test]
fn statfs_reports_backing_source_capacity() {
    let root = unique_mount_test_dir("statfs");
    assert!(ensure_v1_reference_tree(&root).is_ok());

    let stats = mount_statfs_for_source(&root);

    assert!(stats.blocks > 0);
    assert!(stats.files > 0);
    assert!(stats.block_size > 0);
    assert!(stats.name_max > 0);
    assert!(stats.blocks_free <= stats.blocks);
    assert!(stats.blocks_available <= stats.blocks);
    assert!(stats.files_free <= stats.files);
}

#[test]
fn xattrs_describe_virtual_memory_and_disk_backing() {
    let root = unique_mount_test_dir("xattrs");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let fs = CortexFuse::new(root.to_path_buf());
    assert!(fs.is_ok());
    let Ok(fs) = fs else { return };

    let tool = fs.xattrs_for_path("tool/tsh");
    assert!(tool.is_ok());
    let tool = tool.unwrap_or_default();
    assert_eq!(xattr_value(&tool, "user.cortexfs.origin"), Some("virtual"));
    assert_eq!(xattr_value(&tool, "user.cortexfs.storage"), Some("memory"));
    assert_eq!(xattr_value(&tool, "user.cortexfs.virtual"), Some("true"));
    assert_eq!(
        xattr_value(&tool, "user.cortexfs.tokenizer"),
        Some("byte-estimate-v1")
    );
    assert_eq!(xattr_value(&tool, "user.cortexfs.cache_bytes"), Some("0"));
    assert_eq!(xattr_value(&tool, "user.cortexfs.cache_entries"), Some("0"));
    assert_eq!(
        xattr_value(&tool, "user.cortexfs.backing_exists"),
        Some("true")
    );
    assert_eq!(xattr_value(&tool, "user.cortexfs.backing_path"), None);
    let bytes = xattr_value(&tool, "user.cortexfs.bytes")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default();
    let expected_tokens = estimate_tokens_from_bytes(bytes).to_string();
    assert_eq!(
        xattr_value(&tool, "user.cortexfs.token_estimate"),
        Some(expected_tokens.as_str())
    );
    assert_eq!(
        xattr_value(&tool, "user.cortexfs.input_token_estimate"),
        xattr_value(&tool, "user.cortexfs.token_estimate")
    );
    assert_eq!(
        xattr_value(&tool, "user.cortexfs.output_token_estimate"),
        Some("0")
    );

    let schema = fs.xattrs_for_path("tool/tsh.d/schema");
    assert!(schema.is_ok());
    let schema = schema.unwrap_or_default();
    assert_eq!(xattr_value(&schema, "user.cortexfs.origin"), Some("disk"));
    assert_eq!(xattr_value(&schema, "user.cortexfs.storage"), Some("disk"));
    assert_eq!(xattr_value(&schema, "user.cortexfs.virtual"), Some("false"));
    assert_eq!(xattr_value(&schema, "user.cortexfs.backing_path"), None);

    let helper = fs.xattrs_for_path("model/helper");
    assert!(helper.is_ok());
    let helper = helper.unwrap_or_default();
    assert_eq!(xattr_value(&helper, "user.cortexfs.origin"), Some("virtual"));
    assert_eq!(xattr_value(&helper, "user.cortexfs.storage"), Some("memory"));
    assert_eq!(
        xattr_value(&helper, "user.cortexfs.backing_exists"),
        Some("false")
    );

}

fn xattr_value<'a>(attrs: &'a [super::CortexXattr], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find_map(|attr| (attr.name == name).then_some(attr.value.as_str()))
}

struct TestDir(std::path::PathBuf);

impl std::ops::Deref for TestDir {
    type Target = std::path::Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<std::path::Path> for TestDir {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

impl AsRef<std::ffi::OsStr> for TestDir {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.0.as_os_str()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        // ponytail: best-effort test cleanup; stale startup cleanup covers killed test processes.
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

fn unique_mount_test_dir(name: &str) -> TestDir {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    TestDir(std::env::temp_dir().join(format!(
        "cortexfs-mount-{name}-{}-{nanos}",
        std::process::id()
    )))
}
