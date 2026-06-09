use crate::{CortexFs, ROOT_INODE, dir_entry};
use fuse3::FileType;
use fuse3::raw::prelude::{Filesystem, Request};
use std::ffi::OsStr;

#[test]
fn xattr_exposes_cortex_security_context() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let space = fs
        .lookup_path(["home", "1000"])
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let context = fs.node_context(space)?;

    assert_eq!(context, crate::LOCAL_USER_SPACE_CONTEXT_TEXT);
    assert_eq!(
        crate::filesystem::reply_xattr(context.as_bytes(), 0)?,
        fuse3::raw::reply::ReplyXAttr::Size(39)
    );
    assert_eq!(
        crate::filesystem::reply_xattr(context.as_bytes(), 39)?,
        fuse3::raw::reply::ReplyXAttr::Data(bytes::Bytes::copy_from_slice(
            crate::LOCAL_USER_SPACE_CONTEXT_TEXT.as_bytes()
        ))
    );
    assert!(crate::filesystem::reply_xattr(context.as_bytes(), 1).is_err());

    let context_file = fs
        .lookup_path(["home", "1000", "context"])
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert_eq!(fs.node_context(context_file)?, context);

    let status = fs
        .lookup_path(["status"])
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(fs.node_context(status).is_err());

    assert_eq!(
        crate::filesystem::reply_xattr(crate::CORTEX_CONTEXT_XATTR_LIST, 0)?,
        fuse3::raw::reply::ReplyXAttr::Size(20)
    );

    Ok(())
}

#[test]
fn directory_entries_keep_stable_offsets() {
    assert_eq!(
        dir_entry(42, FileType::Directory, "provider", 7),
        fuse3::raw::reply::DirectoryEntry {
            inode: 42,
            kind: FileType::Directory,
            name: "provider".into(),
            offset: 7,
        }
    );
}

#[test]
fn statfs_reports_virtual_read_only_capacity() {
    let fs = CortexFs::new();
    let statfs = fs.statfs_reply();

    assert_eq!(statfs.blocks, crate::STATFS_BLOCKS);
    assert_eq!(statfs.bfree, 0);
    assert_eq!(statfs.bavail, 0);
    assert!(statfs.files > 60);
    assert_eq!(statfs.ffree, 0);
    assert_eq!(statfs.bsize, crate::STATFS_BLOCK_SIZE);
    assert_eq!(statfs.frsize, crate::STATFS_BLOCK_SIZE);
    assert_eq!(statfs.namelen, crate::STATFS_NAME_LENGTH);
}

#[tokio::test]
async fn mkdir_cannot_create_runtime_abi_directories() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let home = fs.path_inode(["home", "1000"])?;

    for (parent, name) in [
        (ROOT_INODE, "chan"),
        (ROOT_INODE, "workflow"),
        (home, "job"),
        (home, "hook"),
    ] {
        let result = fs
            .mkdir(Request::default(), parent, OsStr::new(name), 0o755, 0)
            .await;

        assert_eq!(
            result.map(|_reply| ()),
            Err(fuse3::Errno::from(libc::EROFS))
        );
    }

    assert!(
        fs.lookup_path(["chan"]).is_none(),
        "mkdir must not materialize a second provider/route abstraction"
    );
    assert!(
        fs.lookup_path(["workflow"]).is_none(),
        "mkdir must not materialize a workflow ABI"
    );
    assert!(
        fs.lookup_path(["home", "1000", "job"]).is_none(),
        "mkdir must not materialize a job ABI"
    );
    assert!(
        fs.lookup_path(["home", "1000", "hook"]).is_none(),
        "mkdir must not materialize a hook ABI"
    );
    Ok(())
}
