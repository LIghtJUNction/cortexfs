use crate::{RuntimeState, text::redact_dsn};

impl RuntimeState {
    pub fn write_postgres_dsn_current(&mut self, offset: u64, data: &[u8]) -> fuse3::Result<u32> {
        if offset != 0 {
            return Err(libc::EINVAL.into());
        }
        let dsn = std::str::from_utf8(data).map_err(|_error| libc::EINVAL)?;
        let normalized = format!("{}\n", dsn.trim());
        if let Some(inode) = self.postgres_dsn_current_inode {
            self.update_dynamic_file(inode, normalized.clone());
        }
        let source = if normalized.trim().is_empty() {
            "unset\n"
        } else {
            "current\n"
        };
        let status = if normalized.trim().is_empty() {
            "disabled\n"
        } else {
            "configured\n"
        };
        if let Some(inode) = self.postgres_status_inode {
            self.update_dynamic_file(inode, status);
        }
        if let Some(inode) = self.postgres_dsn_source_inode {
            self.update_dynamic_file(inode, source);
        }
        if let Some(inode) = self.postgres_dsn_effective_inode {
            self.update_dynamic_file(inode, format!("{}\n", redact_dsn(normalized.trim())));
        }
        self.refresh_pgvector_status();
        self.append_audit("database.postgres.dsn", "current", "configured");
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }
}
