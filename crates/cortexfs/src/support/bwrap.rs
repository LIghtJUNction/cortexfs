//! Shared bubblewrap argument builders used by agent, runtime, and tool sandboxes.

/// Create `--dir` args for every absolute path prefix of `cwd`.
#[must_use]
pub fn dir_args_for_chdir(cwd: &str) -> Vec<String> {
    let mut args = Vec::new();
    if !cwd.starts_with('/') {
        return args;
    }
    let mut path = String::new();
    for component in cwd.split('/').filter(|component| !component.is_empty()) {
        path.push('/');
        path.push_str(component);
        args.push("--dir".to_owned());
        args.push(path.clone());
    }
    args
}

/// Create `--dir` args for the parent of an absolute path.
#[must_use]
pub fn dir_args_for_parent(path: &str) -> Vec<String> {
    let Some((parent, _name)) = path.rsplit_once('/') else {
        return Vec::new();
    };
    if parent.is_empty() {
        Vec::new()
    } else {
        dir_args_for_chdir(parent)
    }
}

/// Base host rootfs view shared by agent sandboxes.
///
/// When `unshare_net` is true, inserts `--unshare-net` immediately after
/// `--unshare-pid` (matching historical agent/sandbox argument order).
#[must_use]
pub fn host_rootfs_args(unshare_net: bool) -> Vec<String> {
    let mut args = vec!["--die-with-parent".to_owned(), "--unshare-pid".to_owned()];
    if unshare_net {
        args.push("--unshare-net".to_owned());
    }
    args.extend([
        "--proc".to_owned(),
        "/proc".to_owned(),
        "--dev".to_owned(),
        "/dev".to_owned(),
        "--tmpfs".to_owned(),
        "/tmp".to_owned(),
        "--dir".to_owned(),
        "/run".to_owned(),
        "--dir".to_owned(),
        "/home".to_owned(),
        "--ro-bind".to_owned(),
        "/usr".to_owned(),
        "/usr".to_owned(),
        "--ro-bind".to_owned(),
        "/etc".to_owned(),
        "/etc".to_owned(),
        "--tmpfs".to_owned(),
        "/etc/profile.d".to_owned(),
        "--symlink".to_owned(),
        "usr/bin".to_owned(),
        "/bin".to_owned(),
        "--symlink".to_owned(),
        "usr/lib".to_owned(),
        "/lib".to_owned(),
        "--symlink".to_owned(),
        "usr/lib".to_owned(),
        "/lib64".to_owned(),
    ]);
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_args_for_chdir_builds_prefixes() {
        assert_eq!(
            dir_args_for_chdir("/a/b"),
            vec![
                "--dir".to_owned(),
                "/a".to_owned(),
                "--dir".to_owned(),
                "/a/b".to_owned()
            ]
        );
        assert!(dir_args_for_chdir("relative").is_empty());
        assert!(dir_args_for_parent("/leaf").is_empty());
        assert_eq!(dir_args_for_parent("/a/b/c"), dir_args_for_chdir("/a/b"));
    }
}
