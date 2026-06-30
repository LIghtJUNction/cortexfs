const FALLBACK_STATFS_BLOCKS: u64 = 1024 * 1024;
const FALLBACK_STATFS_USED_BLOCKS: u64 = 1024;
const FALLBACK_STATFS_FREE_BLOCKS: u64 = FALLBACK_STATFS_BLOCKS - FALLBACK_STATFS_USED_BLOCKS;
const FALLBACK_STATFS_FILES: u64 = 1024 * 1024;
const FALLBACK_STATFS_USED_FILES: u64 = 1024;
const FALLBACK_STATFS_FREE_FILES: u64 = FALLBACK_STATFS_FILES - FALLBACK_STATFS_USED_FILES;
const FALLBACK_STATFS_BLOCK_SIZE: u32 = 4096;
const FALLBACK_STATFS_NAME_MAX: u32 = 255;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MountStatfs {
    blocks: u64,
    blocks_free: u64,
    blocks_available: u64,
    files: u64,
    files_free: u64,
    block_size: u32,
    name_max: u32,
    fragment_size: u32,
}

impl MountStatfs {
    const fn fallback() -> Self {
        Self {
            blocks: FALLBACK_STATFS_BLOCKS,
            blocks_free: FALLBACK_STATFS_FREE_BLOCKS,
            blocks_available: FALLBACK_STATFS_FREE_BLOCKS,
            files: FALLBACK_STATFS_FILES,
            files_free: FALLBACK_STATFS_FREE_FILES,
            block_size: FALLBACK_STATFS_BLOCK_SIZE,
            name_max: FALLBACK_STATFS_NAME_MAX,
            fragment_size: FALLBACK_STATFS_BLOCK_SIZE,
        }
    }
}

fn mount_statfs_for_source(source: &Path) -> MountStatfs {
    match statvfs::statvfs(source) {
        Ok(stats) => sanitize_mount_statfs(MountStatfs {
            blocks: stats.blocks(),
            blocks_free: stats.blocks_free(),
            blocks_available: stats.blocks_available(),
            files: stats.files(),
            files_free: stats.files_free(),
            block_size: u32_from_u64(stats.block_size()),
            name_max: u32_from_u64(stats.name_max()),
            fragment_size: u32_from_u64(stats.fragment_size()),
        }),
        Err(_error) => MountStatfs::fallback(),
    }
}

fn sanitize_mount_statfs(stats: MountStatfs) -> MountStatfs {
    let (blocks, blocks_free, blocks_available) = if stats.blocks == 0 {
        (
            FALLBACK_STATFS_BLOCKS,
            FALLBACK_STATFS_FREE_BLOCKS,
            FALLBACK_STATFS_FREE_BLOCKS,
        )
    } else {
        let blocks_free = stats.blocks_free.min(stats.blocks);
        (
            stats.blocks,
            blocks_free,
            stats.blocks_available.min(blocks_free),
        )
    };
    let (files, files_free) = if stats.files == 0 {
        (FALLBACK_STATFS_FILES, FALLBACK_STATFS_FREE_FILES)
    } else {
        (stats.files, stats.files_free.min(stats.files))
    };
    let block_size = stats.block_size.max(1);
    let fragment_size = stats.fragment_size.max(1);
    let name_max = stats.name_max.max(1);
    MountStatfs {
        blocks,
        blocks_free,
        blocks_available,
        files,
        files_free,
        block_size,
        name_max,
        fragment_size,
    }
}

fn u32_from_u64(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
