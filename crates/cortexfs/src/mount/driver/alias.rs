macro_rules! cortexfs_mount_socket_alias_methods {
    () => {
        fn mknod(
            &self,
            req: &Request,
            parent: INodeNo,
            name: &OsStr,
            mode: u32,
            umask: u32,
            rdev: u32,
            reply: ReplyEntry,
        ) {
            let file_type = mode & S_IFMT;
            if file_type == S_IFREG || file_type == 0 {
                let permissions = (mode & 0o7777) & !umask;
                let path = create_session_layout_child_or_reply!(
                    self,
                    req,
                    parent,
                    name,
                    reply,
                    create_layout_file,
                    permissions
                );
                match self.created_layout_node(&path, permissions, req.uid(), req.gid()) {
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
            if FuseProjection::is_socket_alias_path(&path)
                && let Err(error) = self.projection.authorize_socket_alias(&path, req.uid())
            {
                reply.error(errno(error));
                return;
            }
            if FuseProjection::is_socket_alias_path(&path) {
                match self.projection.create_socket_placeholder(
                    &path,
                    req.uid(),
                    req.gid(),
                    (mode & 0o7777) & !umask,
                ) {
                    Ok(node) => self.reply_entry(&node, reply),
                    Err(error) => reply.error(errno(error)),
                }
                return;
            }
            match self.projected_getattr(&path) {
                Ok(_attr) => {
                    reply.error(Errno::EEXIST);
                    return;
                }
                Err(FuseError::NotFound) => {}
                Err(error) => {
                    reply.error(errno(error));
                    return;
                }
            }
            let permissions = (mode & 0o7777) & !umask;
            if self
                .insert_socket_overlay(&path, req.uid(), req.gid(), permissions)
                .is_err()
            {
                reply.error(Errno::EIO);
                return;
            }
            self.reply_entry(
                &socket_node(&path, permissions, req.uid(), req.gid()),
                reply,
            );
        }

        fn unlink(&self, req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
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
            let Some(name_text) = name.to_str() else {
                reply.error(Errno::EINVAL);
                return;
            };
            let parent_path = path_for_inode_or_reply!(self, parent, reply);
            let Some(layout_path) = child_path(&parent_path, name_text) else {
                reply.error(Errno::EINVAL);
                return;
            };
            match self.projection.remove_layout_file(&layout_path, req.uid()) {
                Ok(()) => {
                    if let Err(error) = self.forget_path(&layout_path) {
                        reply.error(errno(error));
                        return;
                    }
                    reply.ok();
                    return;
                }
                Err(FuseError::NotControlFile) => {}
                Err(error) => {
                    reply.error(errno(error));
                    return;
                }
            }
            match self
                .projection
                .remove_socket_alias_claim(&layout_path, req.uid())
            {
                Ok(()) => {
                    if let Err(error) = self.forget_path(&layout_path) {
                        reply.error(errno(error));
                        return;
                    }
                    reply.ok();
                    return;
                }
                Err(FuseError::NotControlFile) => {}
                Err(error) => {
                    reply.error(errno(error));
                    return;
                }
            }
            let path = match self.socket_alias_child_path(parent, name) {
                Ok(path) => path,
                Err(_error) => match self.socket_child_path(parent, name) {
                    Ok(path) => path,
                    Err(error) => {
                        reply.error(errno(error));
                        return;
                    }
                },
            };
            let socket_alias = FuseProjection::is_socket_alias_path(&path);
            if socket_alias
                && let Err(error) = self.projection.authorize_socket_alias(&path, req.uid())
            {
                reply.error(errno(error));
                return;
            }
            let removed_overlay = match self.remove_socket_overlay(&path, req.uid()) {
                Ok(removed) => removed,
                Err(error) => {
                    reply.error(errno(error));
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
            if socket_alias {
                match self.projection.remove_socket_alias(&path, req.uid()) {
                    Ok(()) => {
                        if let Err(error) = self.forget_path(&path) {
                            reply.error(errno(error));
                            return;
                        }
                        reply.ok();
                    }
                    Err(error) => reply.error(errno(error)),
                }
                return;
            }
            match self.projection.getattr(&path) {
                Ok(attr) if attr.file_type() == FuseFileType::Socket => {
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
            req: &Request,
            parent: INodeNo,
            link_name: &OsStr,
            target: &Path,
            reply: ReplyEntry,
        ) {
            if let Ok(path) = self.model_symlink_child_path(parent, link_name) {
                match self.projection.set_model_alias_symlink(&path, target) {
                    Ok(node) => self.reply_entry(&node, reply),
                    Err(error) => reply.error(errno(error)),
                }
                return;
            }
            let path = match self.socket_alias_child_path(parent, link_name) {
                Ok(path) => path,
                Err(_error) => {
                    reply.error(readonly_mutation_errno());
                    return;
                }
            };
            match self
                .projection
                .set_socket_alias(&path, target, req.uid(), req.gid())
            {
                Ok(node) => self.reply_entry(&node, reply),
                Err(error) => reply.error(errno(error)),
            }
        }

        fn rename(
            &self,
            req: &Request,
            parent: INodeNo,
            name: &OsStr,
            newparent: INodeNo,
            newname: &OsStr,
            flags: RenameFlags,
            reply: ReplyEmpty,
        ) {
            if flags != RenameFlags::empty() && flags != RenameFlags::RENAME_NOREPLACE {
                reply.error(Errno::EINVAL);
                return;
            }
            if flags.is_empty()
                && self
                    .rename_model_alias_path(parent, name, newparent, newname)
                    .is_ok()
            {
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
            match self.rename_owner_path(&from, &to, req.uid(), flags) {
                Ok(()) => reply.ok(),
                Err(FuseError::NotControlFile) if flags.is_empty() => {
                    reply.error(readonly_mutation_errno());
                }
                Err(error) => reply.error(errno(error)),
            }
        }
    };
}
