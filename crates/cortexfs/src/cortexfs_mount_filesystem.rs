impl Filesystem for CortexFuse {
    fn forget(&self, _req: &Request, ino: INodeNo, nlookup: u64) {
        let _ignored = self.forget_inode(ino, nlookup);
    }

    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let Some(name) = name.to_str() else {
            reply.error(Errno::EINVAL);
            return;
        };
        let parent_path = path_for_inode_or_reply!(self, parent, reply);
        let parent_node = match self.projected_node_for_path(&parent_path) {
            Ok(node) => node,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        match self.projected_lookup(&parent_node, name) {
            Ok(node) => self.reply_entry(&node, reply),
            Err(error) => reply.error(errno(error)),
        }
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        let path = path_for_inode_or_reply!(self, ino, reply);
        match self.projected_getattr(&path) {
            Ok(attr) => reply.attr(&TTL, &file_attr(ino.0, &attr)),
            Err(error) => reply.error(errno(error)),
        }
    }

    fn access(&self, req: &Request, ino: INodeNo, mask: AccessFlags, reply: ReplyEmpty) {
        let path = path_for_inode_or_reply!(self, ino, reply);
        let attr = match self.projected_getattr(&path) {
            Ok(attr) => attr,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        let groups = supplementary_groups_for_pid(req.pid());
        match access_error(&attr, req.uid(), req.gid(), &groups, mask) {
            Some(error) => reply.error(error),
            None => reply.ok(),
        }
    }

    fn readlink(&self, _req: &Request, ino: INodeNo, reply: ReplyData) {
        let path = path_for_inode_or_reply!(self, ino, reply);
        match self.projection.readlink(&path) {
            Ok(target) => reply.data(target.as_os_str().as_bytes()),
            Err(error) => reply.error(errno(error)),
        }
    }

    fn setattr(
        &self,
        _req: &Request,
        ino: INodeNo,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        atime: Option<TimeOrNow>,
        mtime: Option<TimeOrNow>,
        change_time: Option<SystemTime>,
        _fh: Option<FileHandle>,
        creation_time: Option<SystemTime>,
        status_change_time: Option<SystemTime>,
        backup_time: Option<SystemTime>,
        flags: Option<BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        let changes_metadata = mode.is_some()
            || uid.is_some()
            || gid.is_some()
            || atime.is_some()
            || mtime.is_some()
            || change_time.is_some()
            || creation_time.is_some()
            || status_change_time.is_some()
            || backup_time.is_some()
            || flags.is_some();
        if let Some(error) = fuse_setattr_metadata_error(changes_metadata) {
            reply.error(error);
            return;
        }
        let Some(0) = size else {
            reply.error(Errno::EINVAL);
            return;
        };
        let path = path_for_inode_or_reply!(self, ino, reply);
        let attr = match self.projected_getattr(&path) {
            Ok(attr) => attr,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        if let Some(error) = fuse_write_error(&attr) {
            reply.error(error);
            return;
        }
        if let Err(error) = self.projection.write_control_file_at(&path, 0, b"") {
            reply.error(errno(error));
            return;
        }
        match self.projection.getattr(&path) {
            Ok(attr) => reply.attr(&TTL, &file_attr(ino.0, &attr)),
            Err(error) => reply.error(errno(error)),
        }
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let path = path_for_inode_or_reply!(self, ino, reply);
        let entries = match self.projected_readdir(&path) {
            Ok(entries) => entries,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        let mut rows = vec![
            FuseDirRow::new(ino.0, FileType::Directory, "."),
            FuseDirRow::new(
                match parent_inode(&path, &self.paths) {
                    Ok(inode) => inode,
                    Err(error) => {
                        reply.error(errno(error));
                        return;
                    }
                },
                FileType::Directory,
                "..",
            ),
        ];
        for entry in entries {
            match self.node_for_dir_entry(&path, &entry) {
                Ok(node) => {
                    rows.push(FuseDirRow::new(
                        node.inode(),
                        fuser_file_type(entry.file_type()),
                        entry.name(),
                    ));
                    if let Err(error) = self.remember(&node) {
                        reply.error(errno(error));
                        return;
                    }
                }
                Err(error) => {
                    reply.error(errno(error));
                    return;
                }
            }
        }

        let start = match usize::try_from(offset) {
            Ok(start) => start,
            Err(_error) => {
                reply.ok();
                return;
            }
        };
        for (index, row) in rows.into_iter().enumerate().skip(start) {
            let next_offset = u64::try_from(index + 1).unwrap_or(u64::MAX);
            if reply.add(INodeNo(row.inode), next_offset, row.kind, row.name) {
                break;
            }
        }
        reply.ok();
    }

    cortexfs_mount_readdirplus!();

    fn mkdir(
        &self,
        _req: &Request,
        _parent: INodeNo,
        _name: &OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        reply.error(readonly_mutation_errno());
    }

    fn rmdir(&self, _req: &Request, _parent: INodeNo, _name: &OsStr, reply: ReplyEmpty) {
        reply.error(readonly_mutation_errno());
    }

    fn statfs(&self, _req: &Request, _ino: INodeNo, reply: ReplyStatfs) {
        let stats = mount_statfs_for_source(self.projection.root());
        reply.statfs(
            stats.blocks,
            stats.blocks_free,
            stats.blocks_available,
            stats.files,
            stats.files_free,
            stats.block_size,
            stats.name_max,
            stats.fragment_size,
        );
    }

    fn open(&self, _req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        let path = path_for_inode_or_reply!(self, ino, reply);
        let attr = match self.projected_getattr(&path) {
            Ok(attr) => attr,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        if let Some(error) = fuse_open_error(&attr, flags) {
            reply.error(error);
            return;
        }
        reply.opened(FileHandle(0), FopenFlags::empty());
    }

    fn link(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _newparent: INodeNo,
        _newname: &OsStr,
        reply: ReplyEntry,
    ) {
        reply.error(readonly_mutation_errno());
    }

    fn opendir(&self, _req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        if matches!(flags.acc_mode(), OpenAccMode::O_WRONLY | OpenAccMode::O_RDWR) {
            reply.error(Errno::EISDIR);
            return;
        }
        let path = path_for_inode_or_reply!(self, ino, reply);
        match self.projected_getattr(&path) {
            Ok(attr) if attr.file_type() == FuseV1FileType::Directory => {
                reply.opened(FileHandle(0), FopenFlags::empty());
            }
            Ok(_attr) => reply.error(Errno::ENOTDIR),
            Err(error) => reply.error(errno(error)),
        }
    }

    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        let path = path_for_inode_or_reply!(self, ino, reply);
        match self.projection.read_at(&path, offset, usize_from_u32(size)) {
            Ok(bytes) => reply.data(&bytes),
            Err(error) => reply.error(errno(error)),
        }
    }

    fn write(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        data: &[u8],
        _write_flags: WriteFlags,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyWrite,
    ) {
        let path = path_for_inode_or_reply!(self, ino, reply);
        let attr = match self.projected_getattr(&path) {
            Ok(attr) => attr,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        if let Some(error) = fuse_write_error(&attr) {
            reply.error(error);
            return;
        }
        match self.projection.write_control_file_at(&path, offset, data) {
            Ok(()) => match u32::try_from(data.len()) {
                Ok(count) => reply.written(count),
                Err(_error) => reply.error(Errno::EFBIG),
            },
            Err(error) => reply.error(errno(error)),
        }
    }

    fn flush(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _lock_owner: LockOwner,
        reply: ReplyEmpty,
    ) {
        reply.ok();
    }

    fn fsync(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _datasync: bool,
        reply: ReplyEmpty,
    ) {
        reply.ok();
    }

    fn fsyncdir(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _datasync: bool,
        reply: ReplyEmpty,
    ) {
        reply.ok();
    }

    fn fallocate(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _offset: u64,
        _length: u64,
        _mode: i32,
        reply: ReplyEmpty,
    ) {
        reply.error(readonly_mutation_errno());
    }

    fn lseek(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: i64,
        whence: i32,
        reply: ReplyLseek,
    ) {
        let path = path_for_inode_or_reply!(self, ino, reply);
        let attr = match self.projected_getattr(&path) {
            Ok(attr) => attr,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        match fuse_lseek_offset(&attr, offset, whence) {
            Ok(offset) => reply.offset(offset),
            Err(error) => reply.error(error),
        }
    }

    fn copy_file_range(
        &self,
        _req: &Request,
        _ino_in: INodeNo,
        _fh_in: FileHandle,
        _offset_in: u64,
        _ino_out: INodeNo,
        _fh_out: FileHandle,
        _offset_out: u64,
        _len: u64,
        _flags: CopyFileRangeFlags,
        reply: ReplyWrite,
    ) {
        reply.error(fuse_copy_file_range_error());
    }

    fn bmap(&self, _req: &Request, _ino: INodeNo, _blocksize: u32, _idx: u64, reply: ReplyBmap) {
        reply.error(Errno::EINVAL);
    }

    fn ioctl(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _flags: IoctlFlags,
        _cmd: u32,
        _in_data: &[u8],
        _out_size: u32,
        reply: ReplyIoctl,
    ) {
        reply.error(fuse_ioctl_error());
    }

    fn poll(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        _ph: PollNotifier,
        events: PollEvents,
        _flags: PollFlags,
        reply: ReplyPoll,
    ) {
        let path = path_for_inode_or_reply!(self, ino, reply);
        match self.projected_getattr(&path) {
            Ok(_attr) => reply.poll(events),
            Err(error) => reply.error(errno(error)),
        }
    }

    cortexfs_mount_socket_alias_methods!();

    fn setxattr(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _name: &OsStr,
        _value: &[u8],
        _flags: i32,
        _position: u32,
        reply: ReplyEmpty,
    ) {
        reply.error(Errno::EROFS);
    }

    fn getxattr(&self, _req: &Request, ino: INodeNo, name: &OsStr, size: u32, reply: ReplyXattr) {
        let Some(name) = name.to_str() else {
            reply.error(Errno::EINVAL);
            return;
        };
        let path = path_for_inode_or_reply!(self, ino, reply);
        let attrs = match self.xattrs_for_path(&path) {
            Ok(attrs) => attrs,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        let Some(value) = attrs
            .iter()
            .find_map(|attr| (attr.name == name).then_some(attr.value.as_bytes()))
        else {
            reply.error(Errno::ENODATA);
            return;
        };
        reply_xattr_bytes(value, size, reply);
    }

    fn listxattr(&self, _req: &Request, ino: INodeNo, size: u32, reply: ReplyXattr) {
        let path = path_for_inode_or_reply!(self, ino, reply);
        let attrs = match self.xattrs_for_path(&path) {
            Ok(attrs) => attrs,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        let mut bytes = Vec::new();
        for attr in attrs {
            bytes.extend_from_slice(attr.name.as_bytes());
            bytes.push(0);
        }
        reply_xattr_bytes(&bytes, size, reply);
    }

    fn removexattr(&self, _req: &Request, _ino: INodeNo, _name: &OsStr, reply: ReplyEmpty) {
        reply.error(Errno::EROFS);
    }

    fn create(
        &self,
        _req: &Request,
        _parent: INodeNo,
        _name: &OsStr,
        _mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        reply.error(readonly_mutation_errno());
    }
}
