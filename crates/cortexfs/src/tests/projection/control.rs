#[test]
fn fuse_v1_projection_reads_and_writes_control_files() {
    let root = reference_tree("fuse-v1-projection-control-files");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

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
        Err(FuseV1Error::NotControlFile)
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
        Err(FuseV1Error::InvalidOffset)
    );
    assert_eq!(
        projection.write_control_file_at("agent/coder.d/status", 0, &[0xff]),
        Err(FuseV1Error::InvalidContent)
    );
    assert_eq!(
        projection.write_control_file("../escape", "no\n"),
        Err(FuseV1Error::InvalidPath)
    );
    assert_eq!(
        projection.write_control_file(
            "agent/coder.d/cwd",
            &"x".repeat(MAX_FUSE_V1_SMALL_WRITE_BYTES + 1)
        ),
        Err(FuseV1Error::TooLarge)
    );
    assert_eq!(FuseV1Error::TooLarge.errno(), "EMSGSIZE");
    assert_eq!(FuseV1Error::InvalidOffset.errno(), "EINVAL");
    assert_eq!(FuseV1Error::PermissionDenied.errno(), "EACCES");
    let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    assert_eq!(fuse_metadata_error(&denied), FuseV1Error::PermissionDenied);
    assert_eq!(fuse_readlink_error(&denied), FuseV1Error::PermissionDenied);
}

#[test]
fn fuse_v1_projection_refuses_to_read_symlink_as_file() {
    let root = clean_test_dir("fuse-v1-projection-read-symlink");
    let outside = clean_test_dir("fuse-v1-projection-read-symlink-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    write_text_file(&outside.join("status"), "outside\n");
    assert!(symlink(outside.join("status"), root.join("status")).is_ok());
    let projection = FuseV1Projection::new(&root);

    assert_eq!(
        projection.read_to_string("status"),
        Err(FuseV1Error::NotFile)
    );
    assert_eq!(
        projection.read_at("status", 0, 7),
        Err(FuseV1Error::NotFile)
    );
    assert_file_text(&outside.join("status"), "outside\n");
}

#[test]
fn fuse_v1_projection_refuses_to_read_through_symlink_directory() {
    let root = clean_test_dir("fuse-v1-projection-read-symlink-dir");
    let outside = clean_test_dir("fuse-v1-projection-read-symlink-dir-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    write_text_file(&outside.join("model").join("route"), "fallback: leaked\n");
    assert!(symlink(outside.join("model"), root.join("model")).is_ok());
    let projection = FuseV1Projection::new(&root);

    assert_eq!(
        projection.read_to_string("model/route"),
        Err(FuseV1Error::Io)
    );
    assert_eq!(
        projection.read_at("model/route", 0, 8),
        Err(FuseV1Error::Io)
    );
}

#[test]
fn fuse_v1_projection_refuses_symlink_model_route() {
    let root = clean_test_dir("fuse-v1-projection-route-symlink");
    let outside = clean_test_dir("fuse-v1-projection-route-symlink-outside");
    assert!(fs::create_dir_all(root.join("model")).is_ok());
    write_text_file(&outside.join("route"), "fallback: direct\n");
    assert!(symlink(outside.join("route"), root.join("model").join("route")).is_ok());
    let projection = FuseV1Projection::new(&root);

    assert_eq!(
        projection.read_to_string("model/route"),
        Err(FuseV1Error::Io)
    );
}

#[test]
fn fuse_v1_projection_refuses_to_readdir_symlink_as_directory() {
    let root = clean_test_dir("fuse-v1-projection-readdir-symlink");
    let outside = clean_test_dir("fuse-v1-projection-readdir-symlink-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(outside.join("home")).is_ok());
    write_text_file(&outside.join("home").join("leaked"), "outside\n");
    assert!(symlink(outside.join("home"), root.join("home")).is_ok());
    let projection = FuseV1Projection::new(&root);

    assert_eq!(projection.readdir("home"), Err(FuseV1Error::NotDirectory));
}

#[test]
fn fuse_v1_projection_refuses_to_readdir_through_symlink_directory() {
    let root = clean_test_dir("fuse-v1-projection-readdir-symlink-dir");
    let outside = clean_test_dir("fuse-v1-projection-readdir-symlink-dir-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(outside.join("1000")).is_ok());
    write_text_file(&outside.join("1000").join("leaked"), "outside\n");
    assert!(symlink(&outside, root.join("home")).is_ok());
    let projection = FuseV1Projection::new(&root);

    assert_eq!(projection.readdir("home/1000"), Err(FuseV1Error::Io));
}

#[test]
fn fuse_v1_projection_rejects_symlink_model_directory_for_provider_listing() {
    let root = reference_tree("fuse-v1-model-provider-dir-symlink");
    let outside = clean_test_dir("fuse-v1-model-provider-dir-symlink-outside");
    assert!(fs::remove_dir_all(root.join("model")).is_ok());
    assert!(fs::create_dir_all(outside.join("local")).is_ok());
    assert!(symlink(outside.join("local"), root.join("model")).is_ok());
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    assert_eq!(projection.readdir("model"), Err(FuseV1Error::Io));
}

#[test]
fn fuse_v1_projection_does_not_virtualize_symlink_object_control_dir() {
    let root = clean_test_dir("fuse-v1-symlink-object-control-dir");
    let outside = clean_test_dir("fuse-v1-symlink-object-control-dir-outside");
    assert!(fs::create_dir_all(root.join("agent")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    write_text_file(&root.join("agent").join("coder"), "plain executable\n");
    assert!(symlink(&outside, root.join("agent").join("coder.d")).is_ok());
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    assert_eq!(
        projection.read_to_string("agent/coder"),
        Ok("plain executable\n".to_owned())
    );
}
use super::*;
