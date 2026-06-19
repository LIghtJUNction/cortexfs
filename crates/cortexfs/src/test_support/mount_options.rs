use std::path::PathBuf;
use std::time::Duration;

use crate::{CortexFs, FuseConfig, MountMode, MountOptions, MountSecurityOptions};

#[test]
fn single_user_mount_options_are_hardened_by_default() {
    let options = MountOptions::new("/mnt/cortex");
    let security = options.security();

    assert_eq!(options.mountpoint(), &PathBuf::from("/mnt/cortex"));
    assert_eq!(options.mode(), MountMode::SingleUser);
    assert!(security.default_permissions());
    assert!(!security.allow_other());
    assert!(security.noexec());
    assert!(security.nodev());
    assert!(security.nosuid());
}

#[test]
fn multi_user_mount_options_request_allow_other_without_relaxing_hardening() {
    let options = MountOptions::multi_user("/mnt/cortex");
    let security = options.security();

    assert_eq!(options.mode(), MountMode::MultiUser);
    assert!(security.default_permissions());
    assert!(security.allow_other());
    assert!(security.noexec());
    assert!(security.nodev());
    assert!(security.nosuid());
}

#[test]
fn with_mode_multi_user_updates_security_without_relaxing_hardening() {
    let options = MountOptions::new("/mnt/cortex").with_mode(MountMode::MultiUser);
    let security = options.security();

    assert_eq!(options.mode(), MountMode::MultiUser);
    assert!(security.default_permissions());
    assert!(security.allow_other());
    assert!(security.noexec());
    assert!(security.nodev());
    assert!(security.nosuid());
}

#[test]
fn with_mode_single_user_removes_allow_other_without_relaxing_hardening() {
    let options = MountOptions::multi_user("/mnt/cortex").with_mode(MountMode::SingleUser);
    let security = options.security();

    assert_eq!(options.mode(), MountMode::SingleUser);
    assert!(security.default_permissions());
    assert!(!security.allow_other());
    assert!(security.noexec());
    assert!(security.nodev());
    assert!(security.nosuid());
}

#[test]
fn fuse_config_keeps_mount_options() {
    let options = MountOptions::new("/mnt/cortex");
    let config = FuseConfig::new(options.clone());

    assert_eq!(config.options(), &options);
}

#[test]
fn explicit_security_options_can_be_replaced() {
    let security = MountSecurityOptions::multi_user();
    let options = MountOptions::new("/mnt/cortex").with_security(security);

    assert!(options.security().allow_other());
}

#[test]
fn fuse_mount_options_include_hardening_custom_options() {
    let options = crate::fuse_security_custom_options(MountSecurityOptions::new());

    assert_eq!(
        options.as_deref(),
        Some(std::ffi::OsStr::new("noexec,nodev,nosuid"))
    );
}

#[test]
fn multi_user_fuse_mount_options_keep_hardening_custom_options() {
    let options = crate::fuse_security_custom_options(MountSecurityOptions::multi_user());

    assert_eq!(
        options.as_deref(),
        Some(std::ffi::OsStr::new("noexec,nodev,nosuid"))
    );
}

#[test]
fn multi_user_projection_permissions_allow_cross_user_submits() -> fuse3::Result<()> {
    let single_user_fs = CortexFs::new();
    let multi_user_fs = CortexFs::new_with_mode(MountMode::MultiUser);
    let inbox = multi_user_fs.resolve_path_inode(crate::DEMO_THREAD_INBOX_PATH)?;
    let default_provider = multi_user_fs.resolve_path_inode([
        "home",
        crate::LOCAL_USER_ID,
        "route",
        "default_provider",
    ])?;

    assert_eq!(single_user_fs.node_attr(inbox)?.perm, 0o755);
    assert_eq!(multi_user_fs.node_attr(inbox)?.perm, 0o777);
    assert_eq!(multi_user_fs.node_attr(default_provider)?.perm, 0o666);
    Ok(())
}

#[tokio::test]
async fn mount_shutdown_returns_when_session_ends_before_signal() -> std::io::Result<()> {
    let shutdown = tokio::time::timeout(
        Duration::from_millis(50),
        crate::wait_for_mount_shutdown(std::future::pending(), std::future::ready(Ok(()))),
    )
    .await
    .map_err(|error| std::io::Error::new(std::io::ErrorKind::TimedOut, error))??;

    assert_eq!(shutdown, crate::MountShutdown::SessionEnded);
    Ok(())
}

#[tokio::test]
async fn mount_shutdown_prefers_completed_session_over_signal() -> std::io::Result<()> {
    let shutdown =
        crate::wait_for_mount_shutdown(std::future::ready(Ok(())), std::future::ready(Ok(())))
            .await?;

    assert_eq!(shutdown, crate::MountShutdown::SessionEnded);
    Ok(())
}

#[tokio::test]
async fn mount_shutdown_returns_signal_when_signal_arrives_first() -> std::io::Result<()> {
    let shutdown = tokio::time::timeout(
        Duration::from_millis(50),
        crate::wait_for_mount_shutdown(std::future::ready(Ok(())), std::future::pending()),
    )
    .await
    .map_err(|error| std::io::Error::new(std::io::ErrorKind::TimedOut, error))??;

    assert_eq!(shutdown, crate::MountShutdown::Signal);
    Ok(())
}
