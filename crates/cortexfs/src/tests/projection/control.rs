#[test]
fn fuse_projection_reads_and_writes_control_files() {
    let root = reference_tree("fuse-projection-control-files");
    let projection =
        FuseProjection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    assert_eq!(
        projection.read_to_string("status"),
        Ok("ready\n".to_owned())
    );
    assert_eq!(projection.read_at("status", 1, 3), Ok(b"ead".to_vec()));
    assert_eq!(projection.read_at("status", 128, 8), Ok(Vec::new()));
    assert!(
        projection
            .write_control_file("agent/coder.d/cwd", "/work/project\n")
            .is_ok()
    );
    assert_eq!(
        projection.read_to_string("agent/coder.d/cwd"),
        Ok("/work/project\n".to_owned())
    );

    assert_eq!(
        projection.write_control_file("status", "busy\n"),
        Err(FuseError::NotControlFile)
    );
    assert!(
        projection
            .write_control_file_at("agent/coder.d/status", 0, b"busy\n")
            .is_ok()
    );
    assert_eq!(
        projection.read_to_string("agent/coder.d/status"),
        Ok("busy\n".to_owned())
    );
    assert_eq!(
        projection.write_control_file_at("agent/coder.d/status", 1, b"idle\n"),
        Err(FuseError::InvalidOffset)
    );
    assert_eq!(
        projection.write_control_file_at("agent/coder.d/status", 0, &[0xff]),
        Err(FuseError::InvalidContent)
    );
    assert_eq!(
        projection.write_control_file("../escape", "no\n"),
        Err(FuseError::InvalidPath)
    );
    assert_eq!(
        projection.write_control_file(
            "agent/coder.d/cwd",
            &"x".repeat(MAX_FUSE_SMALL_WRITE_BYTES + 1)
        ),
        Err(FuseError::TooLarge)
    );
    assert_eq!(FuseError::TooLarge.errno(), "EMSGSIZE");
    assert_eq!(FuseError::InvalidOffset.errno(), "EINVAL");
    assert_eq!(FuseError::PermissionDenied.errno(), "EACCES");
    let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    assert_eq!(fuse_metadata_error(&denied), FuseError::PermissionDenied);
    assert_eq!(fuse_readlink_error(&denied), FuseError::PermissionDenied);
}

#[test]
fn fuse_projection_validates_agent_model_window_pair_atomically() {
    let root = reference_tree("fuse-agent-window-pair");
    let uid = 1000;
    let gid = 1000;
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("local.json"),
        r#"{"name":"local","base_url":"http://127.0.0.1/v1","models":["large","tiny","unknown"],"model_limits":{"large":64,"tiny":16}}"#,
    );
    write_text_file(&root.join("agent/coder.d/model"), "local/large\n");
    let session = root.join("home/1000/agent/coder/session/default/messages.jsonl");
    let session_before = fs::read(&session).ok();
    let projection = FuseProjection::new(&root).with_provider_config_dir(&providers);

    assert!(
        projection
            .write_fuse_file_at_for_owner("agent/coder.d/window", 0, b"32\n", uid, gid)
            .is_ok()
    );
    assert_eq!(
        projection.write_fuse_file_at_for_owner(
            "agent/coder.d/model",
            0,
            b"local/tiny\n",
            uid,
            gid,
        ),
        Err(FuseError::InvalidContent)
    );
    assert_eq!(
        projection.write_fuse_file_at_for_owner(
            "agent/coder.d/model",
            0,
            b"local/unknown\n",
            uid,
            gid,
        ),
        Err(FuseError::InvalidContent)
    );
    assert!(
        projection
            .write_fuse_file_at_for_owner("agent/coder.d/window", 0, b"auto\n", uid, gid)
            .is_ok()
    );
    assert_eq!(
        projection.read_to_string("agent/coder.d/window"),
        Ok("auto\n".to_owned())
    );
    assert_eq!(fs::read(&session).ok(), session_before);
    assert!(
        projection
            .write_fuse_file_at_for_owner("agent/coder.d/model", 0, b"local/tiny\n", uid, gid,)
            .is_ok()
    );
    assert_eq!(
        projection.write_fuse_file_at_for_owner("agent/coder.d/window", 0, b"17\n", uid, gid,),
        Err(FuseError::InvalidContent)
    );
    assert!(
        projection
            .write_fuse_file_at_for_owner("agent/coder.d/window", 0, b"auto\n", uid, gid)
            .is_ok()
    );
    assert!(
        projection
            .write_fuse_file_at_for_owner("agent/coder.d/model", 0, b"local/unknown\n", uid, gid,)
            .is_ok()
    );
    assert_eq!(
        projection.write_fuse_file_at_for_owner("agent/coder.d/window", 0, b"1\n", uid, gid,),
        Err(FuseError::InvalidContent)
    );
}

/// 全新 agent 目录逐个物化控制文件时（ctx agent new 的宿主回退路径），
/// 首次写 model/window 不存在成对文件；缺失的 peer 取创建默认值而不是拒绝，
/// 但显式 window 在 model 未知时仍必须 fail closed。
#[test]
fn fuse_projection_allows_model_window_writes_during_fresh_agent_creation() {
    let root = reference_tree("fuse-agent-window-fresh-create");
    let uid = 1000;
    let gid = 1000;
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("local.json"),
        r#"{"name":"local","base_url":"http://127.0.0.1/v1","models":["large"],"model_limits":{"large":64}}"#,
    );
    let projection = FuseProjection::new(&root).with_provider_config_dir(&providers);
    assert!(
        projection
            .create_layout_dir("agent/fresh.d", uid, gid, 0o755)
            .is_ok()
    );
    write_text_file(&root.join("agent/fresh.d/owner"), "1000\n");
    assert!(
        projection
            .write_fuse_file_at_for_owner("agent/fresh.d/model", 0, b"local/large\n", uid, gid)
            .is_ok(),
        "first model write must not require an existing window control"
    );
    fs::remove_file(root.join("agent/fresh.d/model")).ok();
    assert_eq!(
        projection.write_fuse_file_at_for_owner("agent/fresh.d/window", 0, b"32\n", uid, gid),
        Err(FuseError::InvalidContent),
        "explicit window without any model stays rejected"
    );
    assert!(
        projection
            .write_fuse_file_at_for_owner("agent/fresh.d/window", 0, b"auto\n", uid, gid)
            .is_ok(),
        "auto window must be writable before the model control exists"
    );
    assert!(
        projection
            .write_fuse_file_at_for_owner("agent/fresh.d/model", 0, b"local/large\n", uid, gid)
            .is_ok()
    );
}

#[test]
fn fuse_projection_serializes_concurrent_model_and_window_commits() -> Result<(), String> {
    let root = reference_tree("fuse-agent-window-concurrent");
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("local.json"),
        r#"{"name":"local","base_url":"http://127.0.0.1/v1","models":["large","tiny"],"model_limits":{"large":64,"tiny":32}}"#,
    );
    write_text_file(&root.join("agent/coder.d/model"), "local/large\n");
    write_text_file(&root.join("agent/coder.d/window"), "64\n");
    let projection = FuseProjection::new(&root).with_provider_config_dir(&providers);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let model_projection = projection.clone();
    let model_barrier = std::sync::Arc::clone(&barrier);
    let model = std::thread::spawn(move || {
        model_barrier.wait();
        model_projection.write_fuse_file_at_for_owner(
            "agent/coder.d/model",
            0,
            b"local/tiny\n",
            1000,
            1000,
        )
    });
    let window_projection = projection.clone();
    let window_barrier = std::sync::Arc::clone(&barrier);
    let window = std::thread::spawn(move || {
        window_barrier.wait();
        window_projection.write_fuse_file_at_for_owner(
            "agent/coder.d/window",
            0,
            b"auto\n",
            1000,
            1000,
        )
    });
    barrier.wait();
    let model_result = model
        .join()
        .map_err(|_panic| "model commit thread should not panic".to_owned())?;
    let window_result = window
        .join()
        .map_err(|_panic| "window commit thread should not panic".to_owned())?;
    assert_eq!(window_result, Ok(()));

    let model = projection.read_to_string("agent/coder.d/model");
    let window = projection.read_to_string("agent/coder.d/window");
    match model_result {
        Ok(()) => {
            assert_eq!(model.as_deref(), Ok("local/tiny\n"));
            assert_eq!(window.as_deref(), Ok("auto\n"));
        }
        Err(FuseError::InvalidContent) => {
            assert_eq!(model.as_deref(), Ok("local/large\n"));
            assert_eq!(window.as_deref(), Ok("auto\n"));
        }
        Err(other) => return Err(format!("unexpected model commit result: {other:?}")),
    }
    Ok(())
}

#[test]
fn fuse_projection_temp_window_write_uses_agent_pair_lock() {
    let root = reference_tree("fuse-agent-window-temp-lock");
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("local.json"),
        r#"{"name":"local","base_url":"http://127.0.0.1/v1","models":["large"],"model_limits":{"large":64}}"#,
    );
    write_text_file(&root.join("agent/coder.d/model"), "local/large\n");
    let projection = FuseProjection::new(&root).with_provider_config_dir(&providers);
    let temp = "agent/coder.d/.window.tmp-2-2-0";
    assert!(
        projection
            .create_layout_file(temp, 1000, 1000, 0o600)
            .is_ok()
    );
    let control_dir = fs::File::open(root.join("agent/coder.d"));
    assert!(control_dir.is_ok());
    let lock = control_dir.and_then(|dir| {
        nix::fcntl::Flock::lock(dir, nix::fcntl::FlockArg::LockExclusive)
            .map_err(|(_dir, error)| std::io::Error::from(error))
    });
    assert!(lock.is_ok());
    let (reached_sent, reached) = std::sync::mpsc::channel();
    let (completed_sent, completed) = std::sync::mpsc::channel();
    let writer = projection.clone();
    let thread = std::thread::spawn(move || {
        FuseProjection::set_agent_window_lock_hook(reached_sent);
        let result = writer.write_fuse_file_at_for_owner(temp, 0, b"48\n", 1000, 1000);
        let _ignored = completed_sent.send(result);
    });
    assert_eq!(
        reached.recv_timeout(std::time::Duration::from_secs(2)),
        Ok(())
    );
    assert_eq!(
        completed.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    );
    drop(lock);
    assert_eq!(
        completed.recv_timeout(std::time::Duration::from_secs(2)),
        Ok(Ok(()))
    );
    assert!(thread.join().is_ok());
    assert_eq!(
        projection.rename_atomic_temp(temp, "agent/coder.d/window", 1000),
        Ok(())
    );
    assert_eq!(
        projection.read_to_string("agent/coder.d/window"),
        Ok("48\n".to_owned())
    );
}

#[test]
fn fuse_projection_revalidates_window_temp_at_rename_time() {
    let root = reference_tree("fuse-agent-window-rename");
    let uid = 1000;
    let gid = 1000;
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("local.json"),
        r#"{"name":"local","base_url":"http://127.0.0.1/v1","models":["large","tiny"],"model_limits":{"large":64,"tiny":32}}"#,
    );
    write_text_file(&root.join("agent/coder.d/model"), "local/large\n");
    let projection = FuseProjection::new(&root).with_provider_config_dir(&providers);
    let temp = "agent/coder.d/.window.tmp-1-1-0";
    assert!(projection.create_layout_file(temp, uid, gid, 0o600).is_ok());
    assert!(
        projection
            .write_fuse_file_at_for_owner(temp, 0, b"48\n", uid, gid)
            .is_ok()
    );
    write_text_file(&root.join("agent/coder.d/model"), "local/tiny\n");

    assert_eq!(
        projection.rename_atomic_temp(temp, "agent/coder.d/window", uid),
        Err(FuseError::InvalidContent)
    );
    assert_eq!(
        projection.read_to_string("agent/coder.d/window"),
        Ok("auto\n".to_owned())
    );
}

#[test]
fn fuse_projection_refuses_to_read_symlink_as_file() {
    let root = clean_test_dir("fuse-projection-read-symlink");
    let outside = clean_test_dir("fuse-projection-read-symlink-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    write_text_file(&outside.join("status"), "outside\n");
    assert!(symlink(outside.join("status"), root.join("status")).is_ok());
    let projection = FuseProjection::new(&root);

    assert_eq!(projection.read_to_string("status"), Err(FuseError::NotFile));
    assert_eq!(projection.read_at("status", 0, 7), Err(FuseError::NotFile));
    assert_file_text(&outside.join("status"), "outside\n");
}

#[test]
fn fuse_projection_refuses_to_read_through_symlink_directory() {
    let root = clean_test_dir("fuse-projection-read-symlink-dir");
    let outside = clean_test_dir("fuse-projection-read-symlink-dir-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    write_text_file(&outside.join("model").join("route"), "fallback: leaked\n");
    assert!(symlink(outside.join("model"), root.join("model")).is_ok());
    let projection = FuseProjection::new(&root);

    assert_eq!(projection.read_to_string("model/route"), Err(FuseError::Io));
    assert_eq!(projection.read_at("model/route", 0, 8), Err(FuseError::Io));
}

#[test]
fn fuse_projection_refuses_symlink_model_route() {
    let root = clean_test_dir("fuse-projection-route-symlink");
    let outside = clean_test_dir("fuse-projection-route-symlink-outside");
    assert!(fs::create_dir_all(root.join("model")).is_ok());
    write_text_file(&outside.join("route"), "fallback: direct\n");
    assert!(symlink(outside.join("route"), root.join("model").join("route")).is_ok());
    let projection = FuseProjection::new(&root);

    assert_eq!(projection.read_to_string("model/route"), Err(FuseError::Io));
}

#[test]
fn fuse_projection_refuses_to_readdir_symlink_as_directory() {
    let root = clean_test_dir("fuse-projection-readdir-symlink");
    let outside = clean_test_dir("fuse-projection-readdir-symlink-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(outside.join("home")).is_ok());
    write_text_file(&outside.join("home").join("leaked"), "outside\n");
    assert!(symlink(outside.join("home"), root.join("home")).is_ok());
    let projection = FuseProjection::new(&root);

    assert_eq!(projection.readdir("home"), Err(FuseError::NotDirectory));
}

#[test]
fn fuse_projection_refuses_to_readdir_through_symlink_directory() {
    let root = clean_test_dir("fuse-projection-readdir-symlink-dir");
    let outside = clean_test_dir("fuse-projection-readdir-symlink-dir-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(outside.join("1000")).is_ok());
    write_text_file(&outside.join("1000").join("leaked"), "outside\n");
    assert!(symlink(&outside, root.join("home")).is_ok());
    let projection = FuseProjection::new(&root);

    assert_eq!(projection.readdir("home/1000"), Err(FuseError::Io));
}

#[test]
fn fuse_projection_rejects_symlink_model_directory_for_provider_listing() {
    let root = reference_tree("fuse-model-provider-dir-symlink");
    let outside = clean_test_dir("fuse-model-provider-dir-symlink-outside");
    assert!(fs::remove_dir_all(root.join("model")).is_ok());
    assert!(fs::create_dir_all(outside.join("local")).is_ok());
    assert!(symlink(outside.join("local"), root.join("model")).is_ok());
    let projection =
        FuseProjection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    assert_eq!(projection.readdir("model"), Err(FuseError::Io));
}

#[test]
fn fuse_projection_does_not_virtualize_symlink_object_control_dir() {
    let root = clean_test_dir("fuse-symlink-object-control-dir");
    let outside = clean_test_dir("fuse-symlink-object-control-dir-outside");
    assert!(fs::create_dir_all(root.join("agent")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    write_text_file(&root.join("agent").join("coder"), "plain executable\n");
    assert!(symlink(&outside, root.join("agent").join("coder.d")).is_ok());
    let projection =
        FuseProjection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    assert_eq!(
        projection.read_to_string("agent/coder"),
        Ok("plain executable\n".to_owned())
    );
}
use super::*;
