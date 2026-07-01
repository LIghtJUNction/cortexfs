macro_rules! cortexfs_mount_init {
    () => {
        fn init(&mut self, _req: &Request, config: &mut KernelConfig) -> io::Result<()> {
            let _ignored = config.set_max_write(fuse_init_max_write());
            let _ignored = config.set_max_readahead(fuse_init_max_readahead());
            Ok(())
        }
    };
}

fn fuse_init_max_write() -> u32 {
    u32::try_from(MAX_FUSE_V1_SMALL_WRITE_BYTES).unwrap_or(u32::MAX)
}

fn fuse_init_max_readahead() -> u32 {
    u32::try_from(MAX_FUSE_V1_SMALL_READ_BYTES).unwrap_or(u32::MAX)
}
