#[test]
pub(crate) fn fuse_init_limits_kernel_io_windows_to_small_file_abi() {
    assert_eq!(
        super::fuse_init_max_write(),
        u32::try_from(super::MAX_FUSE_V1_SMALL_WRITE_BYTES).unwrap_or(u32::MAX)
    );
    assert_eq!(
        super::fuse_init_max_readahead(),
        u32::try_from(super::MAX_FUSE_V1_SMALL_READ_BYTES).unwrap_or(u32::MAX)
    );
}
