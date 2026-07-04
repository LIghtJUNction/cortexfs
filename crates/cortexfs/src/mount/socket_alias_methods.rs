macro_rules! cortexfs_mount_socket_alias_methods {
    () => {
fn mknod(
    &self,
    _req: &Request,
    parent: INodeNo,
    name: &OsStr,
    mode: u32,
    umask: u32,
    rdev: u32,
    reply: ReplyEntry,
) {
            let file_type = mode & S_IFMT;
            if file_type == S_IFREG || file_type == 0 {
                let Some(name) = name.to_str() else {
                    reply.error(Errno::EINVAL);
                    return;
                };
                let parent_path = path_for_inode_or_reply!(self, parent, reply);
                let Some(path) = child_path(&parent_path, name) else {
                    reply.error(Errno::EINVAL);
                    return;
                };
                if let Err(error) = self.projection.create_session_layout_file(&path) {
                    reply.error(if matches!(error, FuseV1Error::NotControlFile) {
                        readonly_mutation_errno()
                    } else {
                        errno(error)
                    });
                    return;
                }
                match self.projected_node_for_path(&path) {
                    Ok(node) => self.reply_entry(&node, reply),
                    Err(error) => reply.error(errno(error)),
                }
                return;
            }
            if file_type != S_IFSOCK {
                reply.error(readonly_mutation_errno());
                return;
            }
    if rdev != 0 {
        reply.error(Errno::EINVAL);
        return;
    }
    let path = match self.socket_child_path(parent, name) {
        Ok(path) => path,
        Err(_error) => {
            reply.error(readonly_mutation_errno());
            return;
        }
    };
    match self.projected_getattr(&path) {
        Ok(_attr) => {
            reply.error(Errno::EEXIST);
            return;
        }
        Err(FuseV1Error::NotFound) => {}
        Err(error) => {
            reply.error(errno(error));
            return;
        }
    }
    if let Err(_error) = self
        .socket_overlays
        .lock()
        .map_err(|_error| FuseV1Error::Io)
        .map(|mut sockets| {
            sockets.insert(path.clone());
        })
    {
        reply.error(Errno::EIO);
        return;
    }
    let permissions = (mode & 0o7777) & !umask;
    self.reply_entry(&socket_node(&path, permissions), reply);
}

fn unlink(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
    match self.unlink_model_path(parent, name) {
        Ok(true) => {
            reply.ok();
            return;
        }
        Ok(false) => {}
        Err(error) => {
            reply.error(errno(error));
            return;
        }
    }
    let path = match self.socket_child_path(parent, name) {
        Ok(path) => path,
        Err(error) => {
            reply.error(errno(error));
            return;
        }
    };
    let removed_overlay = match self.socket_overlays.lock() {
        Ok(mut sockets) => sockets.remove(&path),
        Err(_error) => {
            reply.error(Errno::EIO);
            return;
        }
    };
    if removed_overlay {
        if let Err(error) = self.forget_path(&path) {
            reply.error(errno(error));
            return;
        }
        reply.ok();
        return;
    }
    match self.projection.getattr(&path) {
        Ok(attr) if attr.file_type() == FuseV1FileType::Socket => {
            if remove_backing_socket_entry(self.projection.root(), &path).is_err() {
                reply.error(Errno::EIO);
                return;
            }
            if let Err(error) = self.forget_path(&path) {
                reply.error(errno(error));
                return;
            }
            reply.ok();
        }
        Ok(_attr) => reply.error(readonly_mutation_errno()),
        Err(error) => reply.error(errno(error)),
    }
}

fn symlink(
    &self,
    _req: &Request,
    parent: INodeNo,
    link_name: &OsStr,
    target: &Path,
    reply: ReplyEntry,
) {
    let path = match self.model_symlink_child_path(parent, link_name) {
        Ok(path) => path,
        Err(_error) => {
            reply.error(readonly_mutation_errno());
            return;
        }
    };
    match self.projection.set_model_alias_symlink(&path, target) {
        Ok(node) => self.reply_entry(&node, reply),
        Err(error) => reply.error(errno(error)),
    }
}

        fn rename(
            &self,
            _req: &Request,
    parent: INodeNo,
    name: &OsStr,
    newparent: INodeNo,
    newname: &OsStr,
    flags: RenameFlags,
            reply: ReplyEmpty,
        ) {
            if !flags.is_empty() {
                reply.error(Errno::EINVAL);
                return;
            }
            if self.rename_model_alias_path(parent, name, newparent, newname).is_ok() {
                reply.ok();
                return;
            }
            let Some(name) = name.to_str() else {
                reply.error(Errno::EINVAL);
                return;
            };
            let Some(newname) = newname.to_str() else {
                reply.error(Errno::EINVAL);
                return;
            };
            let from_parent = path_for_inode_or_reply!(self, parent, reply);
            let to_parent = path_for_inode_or_reply!(self, newparent, reply);
            let Some(from) = child_path(&from_parent, name) else {
                reply.error(Errno::EINVAL);
                return;
            };
            let Some(to) = child_path(&to_parent, newname) else {
                reply.error(Errno::EINVAL);
                return;
            };
            match self.projection.rename_session_atomic_temp(&from, &to) {
                Ok(()) => match self.rename_path(&from, &to) {
                    Ok(()) => reply.ok(),
                    Err(error) => reply.error(errno(error)),
                },
                Err(FuseV1Error::NotControlFile) => reply.error(readonly_mutation_errno()),
                Err(error) => reply.error(errno(error)),
            }
        }
    };
}
