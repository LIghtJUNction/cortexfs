#[test]
fn atomic_replace_text_with_mode_replaces_content_and_mode() -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("state.txt");
    fs::write(&path, "old")?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;

    atomic_replace_text_with_mode(&path, "new\n", 0o600)?;

    assert_eq!(fs::read_to_string(&path)?, "new\n");
    assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
    Ok(())
}

#[test]
fn atomic_replace_text_with_mode_ignores_restrictive_umask() -> std::io::Result<()> {
    const CHILD_ENV: &str = "CORTEXFS_TEST_ATOMIC_REPLACE_UMASK_CHILD";
    if std::env::var_os(CHILD_ENV).is_some() {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("state.txt");
        fs::write(&path, "old")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;

        atomic_replace_text_with_mode(&path, "new\n", 0o644)?;

        assert_eq!(fs::read_to_string(&path)?, "new\n");
        assert_eq!(fs::metadata(path)?.permissions().mode() & 0o7777, 0o644);
        return Ok(());
    }

    let test_binary = std::env::current_exe()?;
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg("umask 077; exec \"$1\" tests::atomic_replace_text_with_mode_ignores_restrictive_umask --exact")
        .arg("sh")
        .arg(test_binary)
        .env(CHILD_ENV, "1")
        .status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn atomic_replace_text_preserves_regular_file_metadata() -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    for mode in [0o600, 0o644] {
        let path = temp.path().join(format!("state-{mode:o}.txt"));
        fs::write(&path, "old\n")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(mode))?;
        let before = fs::symlink_metadata(&path)?;

        atomic_replace_text_preserving_metadata(&path, "new\n")?;

        let after = fs::symlink_metadata(&path)?;
        assert_eq!(fs::read_to_string(&path)?, "new\n");
        assert_ne!(after.ino(), before.ino());
        assert_eq!(after.permissions().mode() & 0o7777, mode);
        assert_eq!(after.uid(), before.uid());
        assert_eq!(after.gid(), before.gid());
    }
    Ok(())
}

#[test]
fn atomic_replace_text_preserving_metadata_ignores_restrictive_umask() -> std::io::Result<()> {
    const CHILD_ENV: &str = "CORTEXFS_TEST_ATOMIC_PRESERVE_UMASK_CHILD";
    if std::env::var_os(CHILD_ENV).is_some() {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("state.txt");
        fs::write(&path, "old\n")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;

        atomic_replace_text_preserving_metadata(&path, "new\n")?;

        assert_eq!(fs::metadata(path)?.permissions().mode() & 0o7777, 0o644);
        return Ok(());
    }
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg("umask 077; exec \"$1\" tests::atomic_replace_text_preserving_metadata_ignores_restrictive_umask --exact")
        .arg("sh")
        .arg(std::env::current_exe()?)
        .env(CHILD_ENV, "1")
        .status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn atomic_replace_text_preserving_metadata_refuses_readonly_target() -> std::io::Result<()> {
    const CHILD_ENV: &str = "CORTEXFS_TEST_ATOMIC_READONLY_CHILD";
    const PATH_ENV: &str = "CORTEXFS_TEST_ATOMIC_READONLY_PATH";
    if std::env::var_os(CHILD_ENV).is_some() {
        let path = std::env::var_os(PATH_ENV)
            .map(PathBuf::from)
            .ok_or_else(|| std::io::Error::other("missing readonly fixture path"))?;
        assert_atomic_replace_refuses_readonly_target(&path)?;
        return Ok(());
    }

    if nix::unistd::Uid::effective().is_root() {
        use std::os::unix::process::CommandExt as _;

        const UID: u32 = 65_534;
        const GID: u32 = 65_534;
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("readonly.txt");
        fs::write(&path, "old\n")?;
        nix::unistd::chown(
            temp.path(),
            Some(nix::unistd::Uid::from_raw(UID)),
            Some(nix::unistd::Gid::from_raw(GID)),
        )
        .map_err(std::io::Error::from)?;
        nix::unistd::chown(
            &path,
            Some(nix::unistd::Uid::from_raw(UID)),
            Some(nix::unistd::Gid::from_raw(GID)),
        )
        .map_err(std::io::Error::from)?;
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444))?;

        let status = std::process::Command::new("/proc/self/exe")
            .arg("tests::atomic_replace_text_preserving_metadata_refuses_readonly_target")
            .arg("--exact")
            .env(CHILD_ENV, "1")
            .env(PATH_ENV, &path)
            .uid(UID)
            .gid(GID)
            .status()?;
        assert!(status.success());
        return Ok(());
    }

    let temp = tempfile::tempdir()?;
    let path = temp.path().join("readonly.txt");
    fs::write(&path, "old\n")?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444))?;
    assert_atomic_replace_refuses_readonly_target(&path)
}

#[test]
fn atomic_create_text_does_not_replace_existing_entry() -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("existing.txt");
    fs::write(&path, "keep\n")?;
    let before = fs::symlink_metadata(&path)?;

    let result = atomic_create_text_with_mode(&path, "bad\n", 0o600);

    assert!(matches!(result, Err(ref error) if error.kind() == std::io::ErrorKind::AlreadyExists));
    let after = fs::symlink_metadata(&path)?;
    assert_eq!(fs::read_to_string(path)?, "keep\n");
    assert_eq!(after.ino(), before.ino());
    Ok(())
}

#[test]
fn atomic_replace_text_preserving_metadata_rejects_fifo_without_blocking() -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("pipe");
    nix::unistd::mkfifo(&path, nix::sys::stat::Mode::from_bits_truncate(0o600))
        .map_err(std::io::Error::from)?;

    let started = std::time::Instant::now();
    let result = atomic_replace_text_preserving_metadata(&path, "bad\n");

    assert!(result.is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    assert!(fs::symlink_metadata(path)?.file_type().is_fifo());
    Ok(())
}

fn assert_atomic_replace_refuses_readonly_target(path: &Path) -> std::io::Result<()> {
    let before = fs::symlink_metadata(path)?;

    let result = atomic_replace_text_preserving_metadata(path, "new\n");
    assert!(result.is_err());
    let Err(error) = result else { return Ok(()) };

    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    let after = fs::symlink_metadata(path)?;
    assert_eq!(fs::read_to_string(path)?, "old\n");
    assert_eq!(after.ino(), before.ino());
    assert_eq!(after.permissions().mode(), before.permissions().mode());
    assert_eq!(after.uid(), before.uid());
    assert_eq!(after.gid(), before.gid());
    Ok(())
}

#[test]
fn atomic_replace_text_preserving_metadata_refuses_symlink() -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let target = temp.path().join("target.txt");
    let link = temp.path().join("link.txt");
    fs::write(&target, "keep\n")?;
    assert!(symlink(&target, &link).is_ok());

    assert!(atomic_replace_text_preserving_metadata(&link, "bad\n").is_err());

    assert_eq!(fs::read_to_string(target)?, "keep\n");
    assert!(link.symlink_metadata()?.file_type().is_symlink());
    Ok(())
}

#[test]
fn atomic_replace_text_preserves_foreign_owner_when_root() -> std::io::Result<()> {
    if !nix::unistd::Uid::effective().is_root() {
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("foreign.txt");
    fs::write(&path, "old\n")?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640))?;
    nix::unistd::chown(
        &path,
        Some(nix::unistd::Uid::from_raw(12345)),
        Some(nix::unistd::Gid::from_raw(12346)),
    )
    .map_err(std::io::Error::from)?;

    atomic_replace_text_preserving_metadata(&path, "new\n")?;

    let metadata = fs::symlink_metadata(path)?;
    assert_eq!(metadata.uid(), 12345);
    assert_eq!(metadata.gid(), 12346);
    assert_eq!(metadata.permissions().mode() & 0o7777, 0o640);
    Ok(())
}

#[test]
fn atomic_replace_text_refuses_target_identity_swap_before_commit() -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("state.txt");
    let backup = temp.path().join("state.backup");
    fs::write(&path, "authorized\n")?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640))?;
    let decoy_before = std::cell::Cell::new(None);
    let mut hook = || {
        fs::rename(&path, &backup)?;
        fs::write(&path, "decoy\n")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o604))?;
        let metadata = fs::symlink_metadata(&path)?;
        decoy_before.set(Some((
            metadata.ino(),
            metadata.permissions().mode() & 0o7777,
            metadata.uid(),
            metadata.gid(),
        )));
        Ok(())
    };

    let result = atomic_replace_text_preserving_metadata_with_hook(&path, "replacement\n", &mut hook);
    assert!(result.is_err());

    let decoy = fs::symlink_metadata(&path)?;
    assert_eq!(fs::read_to_string(&path)?, "decoy\n");
    assert_eq!(
        Some((
            decoy.ino(),
            decoy.permissions().mode() & 0o7777,
            decoy.uid(),
            decoy.gid(),
        )),
        decoy_before.get()
    );
    assert_eq!(fs::read_to_string(backup)?, "authorized\n");
    assert!(!fs::read_dir(temp.path())?.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(".state.txt.tmp-"))
    }));
    Ok(())
}

#[test]
fn atomic_replace_text_with_mode_rejects_symlink_parent_without_writing_target(
) -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let link_parent = temp.path().join("state");
    assert!(symlink(outside.path(), &link_parent).is_ok());

    let result = atomic_replace_text_with_mode(&link_parent.join("index"), "new\n", 0o600);

    assert!(result.is_err());
    assert!(!outside.path().join("index").exists());
    Ok(())
}

#[test]
fn atomic_replace_text_with_mode_rejects_symlink_intermediate_dir_without_writing_target(
) -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let outside_nested = outside.path().join("nested");
    fs::create_dir_all(&outside_nested)?;
    let outside_index = outside_nested.join("index");
    fs::write(&outside_index, "outside\n")?;
    let link_parent = temp.path().join("state");
    assert!(symlink(outside.path(), &link_parent).is_ok());

    let result = atomic_replace_text_with_mode(&link_parent.join("nested").join("index"), "new\n", 0o600);

    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&outside_index)?, "outside\n");
    Ok(())
}

#[test]
fn atomic_replace_text_with_mode_replaces_symlink_without_writing_target(
) -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let target = outside.path().join("secret");
    let link = temp.path().join("state.txt");
    fs::write(&target, "target\n")?;
    assert!(symlink(&target, &link).is_ok());

    atomic_replace_text_with_mode(&link, "new\n", 0o600)?;

    assert_eq!(fs::read_to_string(&target)?, "target\n");
    assert_eq!(fs::read_to_string(&link)?, "new\n");
    assert!(link.symlink_metadata()?.is_file());
    Ok(())
}

#[test]
fn append_jsonl_line_appends_newline_to_plain_file() -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("events.jsonl");
    fs::write(&path, "old\n")?;

    append_jsonl_line(&path, "{\"type\":\"done\"}")?;

    assert_eq!(fs::read_to_string(&path)?, "old\n{\"type\":\"done\"}\n");
    Ok(())
}

#[test]
fn append_jsonl_line_rejects_symlink_parent_without_writing_target() -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let link_parent = temp.path().join("session");
    let outside_events = outside.path().join("events.jsonl");
    fs::write(&outside_events, "outside\n")?;
    assert!(symlink(outside.path(), &link_parent).is_ok());

    let result = append_jsonl_line(&link_parent.join("events.jsonl"), "{\"type\":\"done\"}");

    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&outside_events)?, "outside\n");
    Ok(())
}

#[test]
fn append_jsonl_line_rejects_symlink_intermediate_dir_without_writing_target(
) -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let outside_session = outside.path().join("default");
    fs::create_dir_all(&outside_session)?;
    let outside_events = outside_session.join("events.jsonl");
    fs::write(&outside_events, "outside\n")?;
    let link_parent = temp.path().join("sessions");
    assert!(symlink(outside.path(), &link_parent).is_ok());

    let result = append_jsonl_line(
        &link_parent.join("default").join("events.jsonl"),
        "{\"type\":\"done\"}",
    );

    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&outside_events)?, "outside\n");
    Ok(())
}

#[test]
fn append_jsonl_line_refuses_non_regular_targets() {
    let result = append_jsonl_line(Path::new("/dev/null"), "{\"type\":\"done\"}");

    assert!(result.is_err());
}
