use std::sync::Mutex;

static TEST_CWD_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn absolute_existing_path_resolves_relative_mountpoints() {
    let _guard = TEST_CWD_LOCK.lock().expect("cwd lock");
    let root = clean_test_dir("relative-mountpoint");
    let mountpoint = root.join("mnt");
    assert!(fs::create_dir_all(&mountpoint).is_ok());

    let cwd = std::env::current_dir().expect("current directory");
    std::env::set_current_dir(&root).expect("enter fixture directory");
    let resolved = absolute_existing_path(Path::new("mnt"));
    std::env::set_current_dir(cwd).expect("restore current directory");

    assert_eq!(resolved.expect("resolved path"), mountpoint);
}

#[test]
fn is_mount_point_accepts_relative_paths() {
    let _guard = TEST_CWD_LOCK.lock().expect("cwd lock");
    let cwd = std::env::current_dir().expect("current directory");
    std::env::set_current_dir("/").expect("enter filesystem root");
    let mounted = is_mount_point(Path::new("proc"));
    std::env::set_current_dir(cwd).expect("restore current directory");

    assert_eq!(
        mounted.expect("read mountinfo"),
        is_mount_point(Path::new("/proc")).expect("read mountinfo")
    );
}
