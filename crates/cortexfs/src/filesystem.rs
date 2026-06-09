use crate::{
    CORTEX_CONTEXT_XATTR, CORTEX_CONTEXT_XATTR_LIST, CortexFs, MAX_WRITE, SubmissionDirectoryKind,
    TTL,
};
use bytes::Bytes;
use fuse3::raw::prelude::{
    DirectoryEntry, DirectoryEntryPlus, Filesystem, ReplyAttr, ReplyData, ReplyDirectory,
    ReplyDirectoryPlus, ReplyEntry, ReplyInit, ReplyOpen, ReplyStatFs, ReplyWrite, Request,
};
use fuse3::raw::reply::{ReplyCreated, ReplyXAttr};
use fuse3::{FileType, Inode};
use futures_util::stream;
use std::ffi::OsStr;
use std::num::NonZeroU32;

impl Filesystem for CortexFs {
    async fn init(&self, _req: Request) -> fuse3::Result<ReplyInit> {
        let max_write = NonZeroU32::new(MAX_WRITE).ok_or(libc::EINVAL)?;
        Ok(ReplyInit { max_write })
    }

    async fn destroy(&self, _req: Request) {}

    async fn lookup(
        &self,
        _req: Request,
        parent: Inode,
        name: &OsStr,
    ) -> fuse3::Result<ReplyEntry> {
        let node = self.lookup_child(parent, name)?;
        let inode = node.inode();
        Ok(ReplyEntry {
            ttl: TTL,
            attr: self.node_attr(inode)?,
            generation: 0,
        })
    }

    async fn getattr(
        &self,
        _req: Request,
        inode: Inode,
        _fh: Option<u64>,
        _flags: u32,
    ) -> fuse3::Result<ReplyAttr> {
        Ok(ReplyAttr {
            ttl: TTL,
            attr: self.node_attr(inode)?,
        })
    }

    async fn open(&self, _req: Request, inode: Inode, _flags: u32) -> fuse3::Result<ReplyOpen> {
        if self.node_attr(inode)?.kind == FileType::Directory {
            return Err(fuse3::Errno::new_is_dir());
        }
        Ok(ReplyOpen { fh: 0, flags: 0 })
    }

    async fn read(
        &self,
        _req: Request,
        inode: Inode,
        _fh: u64,
        offset: u64,
        size: u32,
    ) -> fuse3::Result<ReplyData> {
        let content = self.node_content(inode)?;
        let start = usize::try_from(offset).map_err(|_error| fuse3::Errno::from(libc::EINVAL))?;
        let read_size = usize::try_from(size).map_err(|_error| fuse3::Errno::from(libc::EINVAL))?;
        let data = content.as_bytes();
        if start >= data.len() {
            return Ok(ReplyData { data: Bytes::new() });
        }
        let end = start.saturating_add(read_size).min(data.len());
        let Some(read_data) = data.get(start..end) else {
            return Err(fuse3::Errno::from(libc::EINVAL));
        };
        Ok(ReplyData {
            data: Bytes::copy_from_slice(read_data),
        })
    }

    async fn create(
        &self,
        _req: Request,
        parent: Inode,
        name: &OsStr,
        _mode: u32,
        _flags: u32,
    ) -> fuse3::Result<ReplyCreated> {
        let name = name.to_str().ok_or(libc::EINVAL)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        let inode = if self.collab_claim_location(parent).is_some() {
            runtime.create_collab_claim(parent, name)?
        } else if self.collab_lock_location(parent).is_some() {
            runtime.create_collab_lock_lease(parent, name)?
        } else {
            let location = self.submission_location(parent).ok_or(libc::EROFS)?;
            if !matches!(location.kind, SubmissionDirectoryKind::Inbox) {
                return Err(libc::EROFS.into());
            }
            runtime.create_staged(parent, location.format, name)?
        };
        let attr = runtime
            .node(inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?
            .attr(self.owner_uid, self.owner_gid);
        drop(runtime);
        Ok(ReplyCreated {
            ttl: TTL,
            attr,
            generation: 0,
            fh: 0,
            flags: 0,
        })
    }

    async fn mkdir(
        &self,
        _req: Request,
        _parent: Inode,
        _name: &OsStr,
        _mode: u32,
        _umask: u32,
    ) -> fuse3::Result<ReplyEntry> {
        Err(libc::EROFS.into())
    }

    async fn write(
        &self,
        _req: Request,
        inode: Inode,
        _fh: u64,
        offset: u64,
        data: &[u8],
        _write_flags: u32,
        _flags: u32,
    ) -> fuse3::Result<ReplyWrite> {
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        let written = runtime.write(inode, offset, data)?;
        drop(runtime);
        Ok(ReplyWrite { written })
    }

    async fn setxattr(
        &self,
        _req: Request,
        _inode: Inode,
        _name: &OsStr,
        _value: &[u8],
        _flags: u32,
        _position: u32,
    ) -> fuse3::Result<()> {
        Err(libc::EROFS.into())
    }

    async fn getxattr(
        &self,
        _req: Request,
        inode: Inode,
        name: &OsStr,
        size: u32,
    ) -> fuse3::Result<ReplyXAttr> {
        if name != CORTEX_CONTEXT_XATTR {
            return Err(libc::ENODATA.into());
        }
        let context = self.node_context(inode)?;
        reply_xattr(context.as_bytes(), size)
    }

    async fn listxattr(&self, _req: Request, inode: Inode, size: u32) -> fuse3::Result<ReplyXAttr> {
        self.node_context(inode)?;
        reply_xattr(CORTEX_CONTEXT_XATTR_LIST, size)
    }

    async fn removexattr(&self, _req: Request, _inode: Inode, _name: &OsStr) -> fuse3::Result<()> {
        Err(libc::EROFS.into())
    }

    async fn rename(
        &self,
        _req: Request,
        parent: Inode,
        name: &OsStr,
        new_parent: Inode,
        new_name: &OsStr,
    ) -> fuse3::Result<()> {
        let name = name.to_str().ok_or(libc::EINVAL)?;
        let new_name = new_name.to_str().ok_or(libc::EINVAL)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        if self.collab_claim_location(new_parent).is_some() {
            return runtime.submit_collab_claim(parent, name, new_parent, new_name);
        }
        if self.collab_lock_location(new_parent).is_some() {
            return runtime.submit_collab_lock_lease(parent, name, new_parent, new_name);
        }
        let submission = self.api_submission(new_parent).ok_or(libc::EROFS)?;
        runtime.submit(parent, name, new_parent, new_name, submission)
    }

    async fn statfs(&self, _req: Request, _inode: Inode) -> fuse3::Result<ReplyStatFs> {
        Ok(self.statfs_reply())
    }

    async fn opendir(&self, _req: Request, inode: Inode, _flags: u32) -> fuse3::Result<ReplyOpen> {
        if !self.is_dir(inode) {
            return Err(fuse3::Errno::new_is_not_dir());
        }
        Ok(ReplyOpen { fh: 0, flags: 0 })
    }

    async fn readdir(
        &self,
        _req: Request,
        parent: Inode,
        _fh: u64,
        offset: i64,
    ) -> fuse3::Result<
        ReplyDirectory<impl futures_util::Stream<Item = fuse3::Result<DirectoryEntry>> + Send + '_>,
    > {
        let skip = usize::try_from(offset.max(0)).map_err(|_error| libc::EINVAL)?;
        let entries = self.children(parent).into_iter().skip(skip).map(Ok);
        Ok(ReplyDirectory {
            entries: stream::iter(entries),
        })
    }

    async fn readdirplus(
        &self,
        _req: Request,
        parent: Inode,
        _fh: u64,
        offset: u64,
        _lock_owner: u64,
    ) -> fuse3::Result<
        ReplyDirectoryPlus<
            impl futures_util::Stream<Item = fuse3::Result<DirectoryEntryPlus>> + Send + '_,
        >,
    > {
        let skip = usize::try_from(offset).map_err(|_error| libc::EINVAL)?;
        let entries = self.children_plus(parent).into_iter().skip(skip).map(Ok);
        Ok(ReplyDirectoryPlus {
            entries: stream::iter(entries),
        })
    }
}

pub fn reply_xattr(value: &[u8], size: u32) -> fuse3::Result<ReplyXAttr> {
    let value_size = u32::try_from(value.len()).map_err(|_error| libc::EOVERFLOW)?;
    if size == 0 {
        return Ok(ReplyXAttr::Size(value_size));
    }
    if size < value_size {
        return Err(libc::ERANGE.into());
    }
    Ok(ReplyXAttr::Data(Bytes::copy_from_slice(value)))
}
