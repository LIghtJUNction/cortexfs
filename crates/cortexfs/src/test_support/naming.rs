use crate::{CortexFs, ROOT_INODE};
use std::ffi::OsStr;

const ROOT_ABI_NAMES: &[&str] = &[
    "status",
    "capabilities",
    "api",
    "formats",
    "providers",
    "models",
    "home",
    "spaces",
    "agents",
    "clusters",
    "mcp",
    "skills",
    "tools",
    "memory",
    "vector",
    "databases",
    "audit",
    "control",
];

const FORBIDDEN_ROOT_ALIASES: &[&str] = &[
    "ctx_home",
    "current_user",
    "user_home",
    "my",
    "me",
    "default",
];

#[test]
fn root_names_are_plain_abi_entries_without_helper_aliases() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let root = fs
        .tree
        .nodes
        .get(&ROOT_INODE)
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    let names = root
        .children()
        .iter()
        .filter_map(|inode| fs.tree.nodes.get(inode))
        .map(crate::Node::name)
        .collect::<Vec<_>>();

    assert_eq!(names, ROOT_ABI_NAMES);
    for alias in FORBIDDEN_ROOT_ALIASES {
        assert!(
            fs.lookup_child(ROOT_INODE, OsStr::new(alias)).is_err(),
            "root ABI must not expose convenience alias {alias}"
        );
    }
    Ok(())
}

#[test]
fn home_directory_uses_uid_entries_without_index_files() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let home = fs.path_inode(["home"])?;
    let home_node = fs
        .tree
        .nodes
        .get(&home)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let names = home_node
        .children()
        .iter()
        .filter_map(|inode| fs.tree.nodes.get(inode))
        .map(crate::Node::name)
        .collect::<Vec<_>>();

    assert_eq!(names, [crate::LOCAL_USER_ID]);
    for helper in ["count", "list", "current", "path", "default"] {
        assert!(
            fs.lookup_path(["home", helper]).is_none(),
            "home must look like a uid directory, not a control surface"
        );
    }
    Ok(())
}
