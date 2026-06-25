use std::sync::Mutex;

static TEST_CWD_LOCK: Mutex<()> = Mutex::new(());

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
