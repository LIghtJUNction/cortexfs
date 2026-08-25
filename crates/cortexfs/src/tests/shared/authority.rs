#[test]
fn shared_access_authority_requires_mount_linux_permission_and_policy() {
    let root = clean_test_dir("shared-authority-ok");
    let shared = root.join("shared-project-a");
    let file = shared.join("data.txt");
    assert!(fs::create_dir_all(&shared).is_ok());
    write_fixture_file(&file, 0o400);

    let identity = ok!(unix_identity_for(&file));
    let mounts = mount_table_for_source_target(
        "/ctx/shared/project-a",
        &shared,
        "ro",
        "bind,nosuid,nodev,noexec",
    );
    let policy = allow_shared_policy("executor_t", "project-a", SharedAccess::Read);
    let authority = SharedAccessAuthority::new(&identity, &mounts, "executor_t", &policy);

    assert_eq!(
        authorize_shared_access("project-a", &file, SharedAccess::Read, authority),
        Ok(())
    );
}

#[test]
fn shared_access_authority_denies_write_on_read_only_mount() {
    let root = clean_test_dir("shared-authority-ro");
    let shared = root.join("shared-project-a");
    let file = shared.join("data.txt");
    assert!(fs::create_dir_all(&shared).is_ok());
    write_fixture_file(&file, 0o600);

    let identity = ok!(unix_identity_for(&file));
    let mounts =
        mount_table_for_source_target("/ctx/shared/project-a", &shared, "ro", "bind,nosuid,nodev");
    let policy = allow_shared_policy("executor_t", "project-a", SharedAccess::Write);
    let authority = SharedAccessAuthority::new(&identity, &mounts, "executor_t", &policy);

    assert_eq!(
        authorize_shared_access("project-a", &file, SharedAccess::Write, authority),
        Err(SharedAccessDenial::ReadOnlyMount)
    );
    assert_eq!(SharedAccessDenial::ReadOnlyMount.errno(), "EROFS");
}

#[test]
fn shared_access_authority_denies_missing_policy_and_wrong_space() {
    let root = clean_test_dir("shared-authority-policy");
    let shared = root.join("shared-project-a");
    let file = shared.join("data.txt");
    assert!(fs::create_dir_all(&shared).is_ok());
    write_fixture_file(&file, 0o400);

    let identity = ok!(unix_identity_for(&file));
    let mounts =
        mount_table_for_source_target("/ctx/shared/project-a", &shared, "ro", "bind,nosuid,nodev");
    let wrong_mounts =
        mount_table_for_source_target("/ctx/shared/project-b", &shared, "ro", "bind,nosuid,nodev");
    let empty_policy = PolicyV0::parse("");
    let empty_policy = ok!(empty_policy);
    let policy = allow_shared_policy("executor_t", "project-a", SharedAccess::Read);

    assert_eq!(
        authorize_shared_access(
            "project-a",
            &file,
            SharedAccess::Read,
            SharedAccessAuthority::new(&identity, &mounts, "executor_t", &empty_policy),
        ),
        Err(SharedAccessDenial::Policy)
    );
    assert_eq!(
        authorize_shared_access(
            "project-a",
            &file,
            SharedAccess::Read,
            SharedAccessAuthority::new(&identity, &wrong_mounts, "executor_t", &policy),
        ),
        Err(SharedAccessDenial::WrongSharedPath)
    );
    assert_eq!(
        authorize_shared_access(
            "project-a",
            &file,
            SharedAccess::Read,
            SharedAccessAuthority::new(&identity, &MountTable::default(), "executor_t", &policy,),
        ),
        Err(SharedAccessDenial::NotMounted)
    );
}

#[test]
fn shared_access_authority_checks_linux_mode_bits() {
    let root = clean_test_dir("shared-authority-linux");
    let shared = root.join("shared-project-a");
    let file = shared.join("data.txt");
    assert!(fs::create_dir_all(&shared).is_ok());
    write_fixture_file(&file, 0o400);

    let metadata = fs::metadata(&file);
    let metadata = ok!(metadata);
    let other_identity = AgentUnixIdentity::new(
        metadata.uid().saturating_add(1),
        metadata.gid().saturating_add(1),
        [],
    );
    let mounts =
        mount_table_for_source_target("/ctx/shared/project-a", &shared, "ro", "bind,nosuid,nodev");
    let policy = allow_shared_policy("executor_t", "project-a", SharedAccess::Read);
    let authority = SharedAccessAuthority::new(&other_identity, &mounts, "executor_t", &policy);

    assert_eq!(
        authorize_shared_access("project-a", &file, SharedAccess::Read, authority),
        Err(SharedAccessDenial::LinuxPermission)
    );
}
use super::*;
