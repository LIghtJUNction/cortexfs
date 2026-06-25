use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use cortexfs::{ensure_v1_reference_tree, FuseV1Attr, FuseV1FileType, FUSE_V1_ROOT_INODE};
use fuser::{FileType, INodeNo};

use super::{estimate_tokens_from_bytes, file_attr, parent_inode, CortexFuse};

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
fn xattrs_describe_virtual_memory_and_disk_backing() {
    let root = unique_mount_test_dir("xattrs");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let fs = CortexFuse::new(root);
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

fn unique_mount_test_dir(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!(
        "cortexfs-mount-{name}-{}-{nanos}",
        std::process::id()
    ))
}
