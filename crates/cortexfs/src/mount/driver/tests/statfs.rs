use super::super::*;

#[test]
pub(crate) fn statfs_falls_back_to_usable_capacity_when_source_is_missing() {
    let root = super::unique_mount_test_dir("statfs-missing-source");
    let stats = mount_statfs_for_source(&root.join("missing"));

    assert_eq!(stats.blocks, 1024 * 1024);
    assert_eq!(stats.blocks_free, (1024 * 1024) - 1024);
    assert_eq!(stats.blocks_available, stats.blocks_free);
    assert_eq!(stats.files, 1024 * 1024);
    assert_eq!(stats.files_free, (1024 * 1024) - 1024);
    assert_eq!(stats.block_size, 4096);
    assert_eq!(stats.fragment_size, 4096);
    assert_eq!(stats.name_max, 255);
}
