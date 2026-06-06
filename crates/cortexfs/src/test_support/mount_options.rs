use std::path::PathBuf;

use crate::{FuseConfig, MountMode, MountOptions, MountSecurityOptions};

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
