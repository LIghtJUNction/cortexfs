macro_rules! cortexfs_mount_readdirplus {
    () => {
        fn readdirplus(
            &self,
            _req: &Request,
            ino: INodeNo,
            _fh: FileHandle,
            offset: u64,
            mut reply: ReplyDirectoryPlus,
        ) {
            let path = path_for_inode_or_reply!(self, ino, reply);
            let node = match self.projected_node_for_path(&path) {
                Ok(node) => node,
                Err(error) => {
                    reply.error(errno(error));
                    return;
                }
            };
            let parent_path = Path::new(&path)
                .parent()
                .and_then(Path::to_str)
                .unwrap_or("");
            let parent_node = match self.projected_node_for_path(parent_path) {
                Ok(node) => node,
                Err(error) => {
                    reply.error(errno(error));
                    return;
                }
            };
            let entries = match self.projected_readdir(&path) {
                Ok(entries) => entries,
                Err(error) => {
                    reply.error(errno(error));
                    return;
                }
            };
            let mut rows = vec![
                (
                    node.inode(),
                    OsString::from("."),
                    file_attr(node.inode(), node.attr()),
                ),
                (
                    parent_node.inode(),
                    OsString::from(".."),
                    file_attr(parent_node.inode(), parent_node.attr()),
                ),
            ];
            for entry in entries {
                match self.node_for_dir_entry(&path, &entry) {
                    Ok(node) => {
                        if let Err(error) = self.remember_lookup(&node) {
                            reply.error(errno(error));
                            return;
                        }
                        rows.push((
                            node.inode(),
                            OsString::from(entry.name()),
                            file_attr(node.inode(), node.attr()),
                        ));
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
            for (index, (inode, name, attr)) in rows.into_iter().enumerate().skip(start) {
                let next_offset = u64::try_from(index + 1).unwrap_or(u64::MAX);
                if reply.add(
                    INodeNo(inode),
                    next_offset,
                    name,
                    &TTL,
                    &attr,
                    Generation(0),
                ) {
                    break;
                }
            }
            reply.ok();
        }
    };
}
