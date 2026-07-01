fn readonly_mutation_errno() -> Errno {
    Errno::EROFS
}

fn fuse_copy_file_range_error() -> Errno {
    readonly_mutation_errno()
}

fn fuse_ioctl_error() -> Errno {
    Errno::ENOTTY
}

fn access_error(
    attr: &FuseV1Attr,
    uid: u32,
    gid: u32,
    groups: &[u32],
    mask: AccessFlags,
) -> Option<Errno> {
    let known = AccessFlags::R_OK | AccessFlags::W_OK | AccessFlags::X_OK;
    if !(mask - known).is_empty() {
        return Some(Errno::EINVAL);
    }
    if mask.contains(AccessFlags::W_OK)
        && attr.file_type() != FuseV1FileType::Socket
        && !fuse_writable_projection_path(attr.abi_path())
    {
        return Some(Errno::EROFS);
    }
    if uid == 0 {
        return (mask.contains(AccessFlags::X_OK) && attr.mode() & 0o111 == 0)
            .then_some(Errno::EACCES);
    }

    let shift = if uid == attr.uid() {
        6
    } else if gid == attr.gid() || groups.contains(&attr.gid()) {
        3
    } else {
        0
    };
    let allowed = (attr.mode() >> shift) & 0o7;
    let mut required = 0;
    if mask.contains(AccessFlags::R_OK) {
        required |= 0o4;
    }
    if mask.contains(AccessFlags::W_OK) {
        required |= 0o2;
    }
    if mask.contains(AccessFlags::X_OK) {
        required |= 0o1;
    }
    (allowed & required != required).then_some(Errno::EACCES)
}

fn supplementary_groups_for_pid(pid: u32) -> Vec<u32> {
    let Ok(status) = fs::read_to_string(format!("/proc/{pid}/status")) else {
        return Vec::new();
    };
    status
        .lines()
        .find_map(|line| line.strip_prefix("Groups:"))
        .map(|line| {
            line.split_whitespace()
                .filter_map(|group| group.parse::<u32>().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn fuse_open_error(attr: &FuseV1Attr, flags: OpenFlags) -> Option<Errno> {
    let wants_write = matches!(flags.acc_mode(), OpenAccMode::O_WRONLY | OpenAccMode::O_RDWR);
    let wants_truncate = flags.0 & nix::libc::O_TRUNC != 0;
    if attr.file_type() == FuseV1FileType::Directory {
        return (wants_write || wants_truncate).then_some(Errno::EISDIR);
    }
    if attr.file_type() == FuseV1FileType::Socket {
        return Some(Errno::ENXIO);
    }
    if flags.acc_mode() == OpenAccMode::O_RDONLY && wants_truncate {
        return Some(Errno::EACCES);
    }
    if wants_write && !fuse_writable_projection_path(attr.abi_path()) {
        return Some(Errno::EROFS);
    }
    None
}

fn fuse_write_error(attr: &FuseV1Attr) -> Option<Errno> {
    if attr.file_type() == FuseV1FileType::Directory {
        return Some(Errno::EISDIR);
    }
    (!fuse_writable_projection_path(attr.abi_path())).then_some(Errno::EROFS)
}

fn fuse_writable_projection_path(path: &str) -> bool {
    let path = parse_abi_path(path);
    matches!(path, AbiPathKind::ModelRoute) || path.is_writable_control_path()
}

fn fuse_setattr_metadata_error(changes_metadata: bool) -> Option<Errno> {
    changes_metadata.then_some(readonly_mutation_errno())
}

fn fuse_lseek_offset(attr: &FuseV1Attr, offset: i64, whence: i32) -> Result<i64, Errno> {
    let size = i64::try_from(attr.size()).map_err(|_error| Errno::EOVERFLOW)?;
    match whence {
        nix::libc::SEEK_SET | nix::libc::SEEK_CUR => nonnegative_seek(offset),
        nix::libc::SEEK_END => nonnegative_seek(size.checked_add(offset).ok_or(Errno::EOVERFLOW)?),
        nix::libc::SEEK_DATA => {
            if offset < 0 {
                return Err(Errno::EINVAL);
            }
            (offset < size).then_some(offset).ok_or(Errno::ENXIO)
        }
        nix::libc::SEEK_HOLE => {
            if offset < 0 {
                return Err(Errno::EINVAL);
            }
            (offset <= size).then_some(size).ok_or(Errno::ENXIO)
        }
        _ => Err(Errno::EINVAL),
    }
}

fn nonnegative_seek(offset: i64) -> Result<i64, Errno> {
    (offset >= 0).then_some(offset).ok_or(Errno::EINVAL)
}
