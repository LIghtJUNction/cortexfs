fn resolve_fuse_abi_path(root: &Path, abi_path: &str) -> Result<PathBuf, FuseV1Error> {
    let normalized = normalize_fuse_abi_path(abi_path)?;
    let mut resolved = root.to_path_buf();
    for component in Path::new(&normalized).components() {
        match component {
            std::path::Component::Normal(part) => resolved.push(part),
            std::path::Component::CurDir => {}
            std::path::Component::RootDir
            | std::path::Component::ParentDir
            | std::path::Component::Prefix(_) => return Err(FuseV1Error::InvalidPath),
        }
    }
    Ok(resolved)
}

fn normalize_fuse_abi_path(abi_path: &str) -> Result<String, FuseV1Error> {
    if abi_path.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(FuseV1Error::InvalidPath);
    }
    let trimmed = abi_path.trim_start_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return Ok(String::new());
    }
    let mut parts = Vec::new();
    for component in Path::new(trimmed).components() {
        match component {
            std::path::Component::Normal(part) => {
                parts.push(part.to_str().ok_or(FuseV1Error::InvalidPath)?);
            }
            std::path::Component::CurDir => {}
            std::path::Component::RootDir
            | std::path::Component::ParentDir
            | std::path::Component::Prefix(_) => return Err(FuseV1Error::InvalidPath),
        }
    }
    Ok(parts.join("/"))
}

fn model_exec_name(abi_path: &str) -> Option<&str> {
    let model = abi_path.strip_prefix("model/")?;
    is_model_name(model).then_some(model)
}

fn read_bytes_at(content: &[u8], offset: u64, size: usize) -> Result<Vec<u8>, FuseV1Error> {
    let start = usize::try_from(offset).map_err(|_error| FuseV1Error::Io)?;
    if start >= content.len() {
        return Ok(Vec::new());
    }
    let end = start.saturating_add(size).min(content.len());
    content
        .get(start..end)
        .map(<[u8]>::to_vec)
        .ok_or(FuseV1Error::Io)
}

fn fuse_join_child_path(parent: &str, name: &str) -> Result<String, FuseV1Error> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(FuseV1Error::InvalidPath);
    }
    if parent.is_empty() {
        normalize_fuse_abi_path(name)
    } else {
        normalize_fuse_abi_path(&format!("{parent}/{name}"))
    }
}

fn fuse_v1_inode_for_path(abi_path: &str) -> u64 {
    if abi_path.is_empty() {
        return FUSE_V1_ROOT_INODE;
    }
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in abi_path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let inode = hash & 0x7fff_ffff_ffff_ffff;
    if inode == 0 || inode == FUSE_V1_ROOT_INODE {
        FUSE_V1_ROOT_INODE + 1
    } else {
        inode
    }
}

fn fuse_file_type(file_type: fs::FileType) -> FuseV1FileType {
    if file_type.is_dir() {
        FuseV1FileType::Directory
    } else if file_type.is_symlink() {
        FuseV1FileType::Symlink
    } else if file_type.is_socket() {
        FuseV1FileType::Socket
    } else if file_type.is_char_device() || file_type.is_block_device() || file_type.is_fifo() {
        FuseV1FileType::Other
    } else {
        FuseV1FileType::Regular
    }
}

fn fuse_metadata_error(error: &std::io::Error) -> FuseV1Error {
    if error.kind() == std::io::ErrorKind::NotFound {
        FuseV1Error::NotFound
    } else {
        FuseV1Error::Io
    }
}

fn is_fuse_v1_writable_control_path(abi_path: &str) -> bool {
    parse_abi_path(abi_path).is_writable_control_path()
}
