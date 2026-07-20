#[test]
fn shared_access_authority_rejects_symlink_paths() {
    let root = clean_test_dir("shared-authority-symlink-deny");
    let shared = root.join("shared-project-a");
    let outside = root.join("outside-host");
    let link = shared.join("data.txt");
    let target = outside.join("escape.txt");
    write_fixture_file(&target, 0o644);
    assert!(fs::create_dir_all(&shared).is_ok());
    assert!(symlink(&target, &link).is_ok());

    let identity = ok!(unix_identity_for(&target));
    let mounts = mount_table_for_source_target(
        "/ctx/shared/project-a",
        &shared,
        "ro",
        "bind,nosuid,nodev,noexec",
    );
    let policy = allow_shared_policy("coder_t", "project-a", SharedAccess::Read);
    let authority = SharedAccessAuthority::new(&identity, &mounts, "coder_t", &policy);

    assert_eq!(
        authorize_shared_access("project-a", &link, SharedAccess::Read, authority),
        Err(SharedAccessDenial::CannotInspectPath)
    );
}

#[test]
fn shared_access_authority_rejects_symlink_intermediate_paths() {
    let root = clean_test_dir("shared-authority-symlink-intermediate-deny");
    let shared = root.join("shared-project-a");
    let outside = root.join("outside-host");
    let file = shared.join("data.txt");
    write_fixture_file(&outside.join("data.txt"), 0o644);
    assert!(symlink(&outside, &shared).is_ok());

    let identity = ok!(unix_identity_for(&outside.join("data.txt")));
    let mounts = mount_table_for_source_target(
        "/ctx/shared/project-a",
        &shared,
        "ro",
        "bind,nosuid,nodev,noexec",
    );
    let policy = allow_shared_policy("coder_t", "project-a", SharedAccess::Read);
    let authority = SharedAccessAuthority::new(&identity, &mounts, "coder_t", &policy);

    assert_eq!(
        authorize_shared_access("project-a", &file, SharedAccess::Read, authority),
        Err(SharedAccessDenial::CannotInspectPath)
    );
}

#[test]
fn session_access_authority_rejects_symlink_paths() -> Result<(), Box<dyn std::error::Error>> {
    let root = clean_test_dir("session-authority-symlink-deny");
    let home = root.join("home-1000");
    let outside = root.join("outside-host");
    let link = home
        .join("agent")
        .join("coder")
        .join("session")
        .join("default")
        .join("messages.jsonl");
    let target = outside.join("messages.jsonl");
    write_fixture_file(&target, 0o644);
    let Some(parent) = link.parent() else {
        return Err("link path has a parent".into());
    };
    assert!(fs::create_dir_all(parent).is_ok());
    assert!(symlink(&target, &link).is_ok());

    let metadata = fs::metadata(&target)?;
    let identity = AgentUnixIdentity::new(1000, metadata.gid(), []);
    let mounts =
        mount_table_for_source_target("/ctx/home/1000", &home, "ro", "bind,nosuid,nodev,noexec");
    let policy = policy_with_rules(["allow coder_t session:default read"]);
    let authority = SessionAccessAuthority::new(&identity, &mounts, "coder_t", &policy);

    assert_eq!(
        authorize_session_access(&link, SessionAccess::Read, authority),
        Err(SessionAccessDenial::CannotInspectPath)
    );
    Ok(())
}

#[test]
fn session_access_authority_rejects_symlink_intermediate_paths()
-> Result<(), Box<dyn std::error::Error>> {
    let root = clean_test_dir("session-authority-symlink-intermediate-deny");
    let home = root.join("home-1000");
    let outside = root.join("outside-host");
    let session_parent = home.join("agent").join("coder").join("session");
    let file = session_parent.join("default").join("messages.jsonl");
    write_fixture_file(&outside.join("default").join("messages.jsonl"), 0o644);
    let Some(parent) = session_parent.parent() else {
        return Err("session path has a parent".into());
    };
    assert!(fs::create_dir_all(parent).is_ok());
    assert!(symlink(&outside, &session_parent).is_ok());

    let metadata = fs::metadata(outside.join("default").join("messages.jsonl"))?;
    let identity = AgentUnixIdentity::new(1000, metadata.gid(), []);
    let mounts =
        mount_table_for_source_target("/ctx/home/1000", &home, "ro", "bind,nosuid,nodev,noexec");
    let policy = policy_with_rules(["allow coder_t session:default read"]);
    let authority = SessionAccessAuthority::new(&identity, &mounts, "coder_t", &policy);

    assert_eq!(
        authorize_session_access(&file, SessionAccess::Read, authority),
        Err(SessionAccessDenial::CannotInspectPath)
    );
    Ok(())
}
use super::*;
