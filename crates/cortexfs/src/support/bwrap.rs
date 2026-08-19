//! Shared bubblewrap argument builders used by agent, runtime, and tool sandboxes.

use std::path::Path;

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

/// Creates a read-only bind for one existing host path and its parent dirs.
#[must_use]
pub fn readonly_bind_args(path: &Path) -> Vec<String> {
    let value = path.display().to_string();
    let mut args = dir_args_for_parent(&value);
    args.extend(["--ro-bind".to_owned(), value.clone(), value]);
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
