use std::sync::Mutex;

static TEST_CWD_LOCK: Mutex<()> = Mutex::new(());

fn legacy_default_source(parent: &Path) -> PathBuf {
    let legacy = parent.join("v1-root");
    assert!(fs::create_dir_all(&legacy).is_ok());
    assert!(fs::write(legacy.join("sentinel"), "keep\n").is_ok());
    legacy
}

#[test]
fn bootstrap_check_keeps_legacy_default_source_unchanged() {
    let parent = clean_test_dir("ctx-bootstrap-check-legacy");
    let legacy = legacy_default_source(&parent);

    assert!(bootstrap_reference_tree_default(&parent, false, true).is_ok());
    assert_eq!(
        fs::read_to_string(legacy.join("sentinel")).unwrap_or_default(),
        "keep\n"
    );
    assert!(!parent.join("root").exists());
}

#[test]
fn bootstrap_dry_run_keeps_legacy_default_source_unchanged() {
    let parent = clean_test_dir("ctx-bootstrap-dry-legacy");
    let legacy = legacy_default_source(&parent);

    assert!(bootstrap_reference_tree_default(&parent, true, false).is_ok());
    assert_eq!(
        fs::read_to_string(legacy.join("sentinel")).unwrap_or_default(),
        "keep\n"
    );
    assert!(!parent.join("root").exists());
}

#[test]
fn default_source_adoption_returns_canonical_without_creating() {
    let parent = clean_test_dir("ctx-source-adopt-empty");

    assert_eq!(
        adopt_default_source_root(&parent),
        Ok(parent.join("root"))
    );
    assert!(!parent.exists());
}

#[test]
fn default_source_adoption_renames_legacy_plain_directory() {
    let parent = clean_test_dir("ctx-source-adopt-legacy");
    let legacy = parent.join("v1-root");
    assert!(fs::create_dir_all(&legacy).is_ok());
    assert!(fs::write(legacy.join("state"), "kept\n").is_ok());

    assert_eq!(
        adopt_default_source_root(&parent),
        Ok(parent.join("root"))
    );
    assert!(!legacy.exists());
    assert_eq!(
        fs::read_to_string(parent.join("root/state")).unwrap_or_default(),
        "kept\n"
    );
}

#[test]
fn default_source_adoption_rejects_conflicting_roots_without_mutation() {
    let parent = clean_test_dir("ctx-source-adopt-conflict");
    let legacy = parent.join("v1-root");
    let canonical = parent.join("root");
    assert!(fs::create_dir_all(&legacy).is_ok());
    assert!(fs::create_dir_all(&canonical).is_ok());

    assert!(adopt_default_source_root(&parent).is_err());
    assert!(legacy.is_dir());
    assert!(canonical.is_dir());
}

#[test]
fn default_source_adoption_rejects_legacy_symlink_without_following() {
    let parent = clean_test_dir("ctx-source-adopt-symlink");
    let outside = clean_test_dir("ctx-source-adopt-outside");
    assert!(fs::create_dir_all(&parent).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(std::os::unix::fs::symlink(&outside, parent.join("v1-root")).is_ok());

    assert!(adopt_default_source_root(&parent).is_err());
    assert!(parent.join("v1-root").symlink_metadata().is_ok_and(|metadata| metadata.file_type().is_symlink()));
    assert!(!parent.join("root").exists());
}

#[test]
fn default_source_adoption_rejects_legacy_file() {
    let parent = clean_test_dir("ctx-source-adopt-file");
    assert!(fs::create_dir_all(&parent).is_ok());
    assert!(fs::write(parent.join("v1-root"), "keep\n").is_ok());

    assert!(adopt_default_source_root(&parent).is_err());
    assert_eq!(
        fs::read_to_string(parent.join("v1-root")).unwrap_or_default(),
        "keep\n"
    );
    assert!(!parent.join("root").exists());
}

#[test]
fn absolute_existing_path_resolves_relative_mountpoints() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = TEST_CWD_LOCK.lock()?;
    let root = clean_test_dir("relative-mountpoint");
    let mountpoint = root.join("mnt");
    assert!(fs::create_dir_all(&mountpoint).is_ok());

    let cwd = std::env::current_dir()?;
    std::env::set_current_dir(&root)?;
    let resolved = absolute_existing_path(Path::new("mnt"));
    std::env::set_current_dir(cwd)?;

    assert_eq!(resolved?, mountpoint);
    Ok(())
}

#[test]
fn is_mount_point_accepts_relative_paths() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = TEST_CWD_LOCK.lock()?;
    let cwd = std::env::current_dir()?;
    std::env::set_current_dir("/")?;
    let mounted = is_mount_point(Path::new("proc"));
    std::env::set_current_dir(cwd)?;

    assert_eq!(mounted?, is_mount_point(Path::new("/proc"))?);
    Ok(())
}

#[test]
fn mountpoint_plain_dir_check_accepts_plain_directory() {
    let root = clean_test_dir("ctx-mountpoint-plain");
    let mountpoint = root.join("mnt");
    assert!(fs::create_dir_all(&mountpoint).is_ok());

    assert!(ensure_plain_mountpoint_dir(&mountpoint).is_ok());
}

#[test]
fn mountpoint_plain_dir_check_rejects_symlink_directory() {
    let root = clean_test_dir("ctx-mountpoint-symlink");
    let outside = clean_test_dir("ctx-mountpoint-symlink-outside");
    let mountpoint = root.join("mnt");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(std::os::unix::fs::symlink(&outside, &mountpoint).is_ok());

    assert!(ensure_plain_mountpoint_dir(&mountpoint).is_err());
}

#[test]
fn mountpoint_creation_rejects_symlink_intermediate_without_creating_target() {
    let root = clean_test_dir("ctx-mountpoint-create-symlink-intermediate");
    let outside = clean_test_dir("ctx-mountpoint-create-symlink-intermediate-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("link")).is_ok());

    let mountpoint = root.join("link").join("mnt");

    assert!(create_plain_mountpoint_dir(&mountpoint).is_err());
    assert!(!outside.join("mnt").exists());
}

#[test]
fn cortexfs_mount_bin_never_falls_back_to_relative_path() {
    let mount_bin = cortexfs_mount_bin();

    assert!(mount_bin.is_absolute(), "{}", mount_bin.display());
    assert!(mount_bin.ends_with("cortexfs-mount"), "{}", mount_bin.display());
}

#[test]
fn plain_sibling_mount_bin_accepts_plain_executable() {
    let root = clean_test_dir("ctx-plain-sibling-mount-bin");
    let current = root.join("ctx");
    let sibling = root.join("cortexfs-mount");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::write(&current, "").is_ok());
    assert!(fs::write(&sibling, "#!/bin/sh\nexit 0\n").is_ok());
    assert!(fs::set_permissions(&sibling, fs::Permissions::from_mode(0o755)).is_ok());

    assert_eq!(plain_sibling_mount_bin(&current), Some(sibling));
}

#[test]
fn plain_sibling_mount_bin_rejects_symlink_executable() {
    let root = clean_test_dir("ctx-symlink-sibling-mount-bin");
    let current = root.join("ctx");
    let target = root.join("target");
    let sibling = root.join("cortexfs-mount");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::write(&current, "").is_ok());
    assert!(fs::write(&target, "#!/bin/sh\nexit 0\n").is_ok());
    assert!(fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).is_ok());
    assert!(std::os::unix::fs::symlink(&target, &sibling).is_ok());

    assert_eq!(plain_sibling_mount_bin(&current), None);
}

#[test]
fn mount_spawn_commands_use_clean_runtime_environment() {
    let mount_bin = Path::new("/usr/bin/cortexfs-mount");
    let source = Path::new("/var/lib/cortexfs/storage/current");
    let mountpoint = Path::new("/ctx");
    let detached = detached_mount_command(mount_bin, source, mountpoint);
    let direct = direct_mount_command(mount_bin, source, mountpoint);

    assert_eq!(detached.get_program(), "/usr/bin/setsid");
    assert_eq!(
        detached
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        vec![
            "-f".to_owned(),
            "/usr/bin/cortexfs-mount".to_owned(),
            "--source".to_owned(),
            "/var/lib/cortexfs/storage/current".to_owned(),
            "/ctx".to_owned(),
        ]
    );
    assert_eq!(direct.get_program(), "/usr/bin/cortexfs-mount");
    assert_eq!(
        direct
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        vec![
            "--source".to_owned(),
            "/var/lib/cortexfs/storage/current".to_owned(),
            "/ctx".to_owned(),
        ]
    );
    for command in [detached, direct] {
        let mut envs = command
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect::<Vec<_>>();
        envs.sort();
        assert_eq!(
            envs,
            vec![("PATH".to_owned(), Some("/usr/bin:/bin".to_owned()))]
        );
    }
}
