use crate::{Node, RuntimeState, validation::validate_control_write};

impl RuntimeState {
    pub fn add_vector_runtime_files(&mut self, parents: &crate::RuntimeParents) {
        if let Some(inode) = parents.pgvector_enabled {
            self.nodes
                .insert(inode, Node::dynamic_file(inode, "enabled", "0\n"));
        }
        if let Some(inode) = parents.pgvector_status {
            self.nodes
                .insert(inode, Node::dynamic_file(inode, "status", "disabled\n"));
        }
        if let Some(inode) = parents.pgvector_collections {
            self.nodes
                .insert(inode, Node::dynamic_file(inode, "collections", "\n"));
        }
        if let Some(inode) = parents.pgvector_refresh {
            self.nodes
                .insert(inode, Node::dynamic_file(inode, "refresh", ""));
        }
    }

    pub fn write_pgvector_enabled(&mut self, offset: u64, data: &[u8]) -> fuse3::Result<u32> {
        if offset != 0 {
            return Err(libc::EINVAL.into());
        }
        let enabled = std::str::from_utf8(data).map_err(|_error| libc::EINVAL)?;
        let value = match enabled.trim() {
            "0" => "0\n",
            "1" => "1\n",
            _ => return Err(libc::EINVAL.into()),
        };
        if let Some(inode) = self.pgvector_enabled_inode {
            self.update_dynamic_file(inode, value);
        }
        let status = if value == "1\n" {
            "configured\n"
        } else {
            "disabled\n"
        };
        if let Some(inode) = self.pgvector_status_inode {
            self.update_dynamic_file(inode, status);
        }
        self.append_audit("vector.store.pgvector", "enabled", "configured");
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    pub fn write_pgvector_refresh(&mut self, offset: u64, data: &[u8]) -> fuse3::Result<u32> {
        validate_control_write(offset, data)?;
        let enabled = self
            .pgvector_enabled_inode
            .and_then(|inode| self.nodes.get(&inode))
            .and_then(Node::content)
            .is_some_and(|content| content.trim() == "1");
        let status = if enabled { "ready\n" } else { "disabled\n" };
        if let Some(inode) = self.pgvector_status_inode {
            self.update_dynamic_file(inode, status);
        }
        if let Some(inode) = self.pgvector_collections_inode {
            let collections = if enabled { "memory_semantic\n" } else { "\n" };
            self.update_dynamic_file(inode, collections);
        }
        if let Some(inode) = self.pgvector_refresh_inode {
            self.update_dynamic_file(inode, "1\n");
        }
        self.update_dynamic_file(self.last_control_inode, "vector/stores/pgvector/refresh\n");
        self.append_audit("vector.store.pgvector", "refresh", "refreshed");
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }
}
