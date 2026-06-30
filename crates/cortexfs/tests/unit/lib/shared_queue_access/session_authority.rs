#[test]
fn session_access_authority_allows_explicit_im_channel_session() {
    let root = clean_test_dir("session-authority-im-ok");
    let shared = root.join("im-qq-dev");
    let messages = shared
        .join("agent")
        .join("bot")
        .join("session")
        .join("group-456")
        .join("messages.jsonl");
    write_fixture_file(&messages, 0o600);

    let identity = ok!(unix_identity_for(&messages));
    let mounts = mount_table_for_source_target(
        "/ctx/shared/im-qq-dev",
        &shared,
        "ro",
        "bind,nosuid,nodev,noexec",
    );
    let policy = policy_with_rules([
        "allow bot_t shared:im-qq-dev read",
        "allow bot_t session:group-456 read",
    ]);
    let authority = SessionAccessAuthority::new(&identity, &mounts, "bot_t", &policy);

    assert_eq!(
        authorize_session_access(&messages, SessionAccess::Read, authority),
        Ok(())
    );
}

#[test]
fn session_access_authority_denies_cross_channel_without_session_policy() {
    let root = clean_test_dir("session-authority-im-deny");
    let shared = root.join("im-qq-dev");
    let allowed = shared
        .join("agent")
        .join("bot")
        .join("session")
        .join("group-456")
        .join("messages.jsonl");
    let other = shared
        .join("agent")
        .join("bot")
        .join("session")
        .join("group-999")
        .join("messages.jsonl");
    write_fixture_file(&allowed, 0o600);
    write_fixture_file(&other, 0o600);

    let identity = ok!(unix_identity_for(&allowed));
    let mounts = mount_table_for_source_target(
        "/ctx/shared/im-qq-dev",
        &shared,
        "ro",
        "bind,nosuid,nodev,noexec",
    );
    let policy = policy_with_rules([
        "allow bot_t shared:im-qq-dev read",
        "allow bot_t session:group-456 read",
    ]);
    let authority = SessionAccessAuthority::new(&identity, &mounts, "bot_t", &policy);

    assert_eq!(
        authorize_session_access(&allowed, SessionAccess::Read, authority),
        Ok(())
    );
    assert_eq!(
        authorize_session_access(&other, SessionAccess::Read, authority),
        Err(SessionAccessDenial::SessionPolicy)
    );
    assert_eq!(SessionAccessDenial::SessionPolicy.errno(), "EACCES");
}

#[test]
fn session_access_authority_requires_shared_policy_and_mount_write_mode() {
    let root = clean_test_dir("session-authority-shared-policy");
    let shared = root.join("im-slack-company");
    let messages = shared
        .join("agent")
        .join("bot")
        .join("session")
        .join("channel-789")
        .join("messages.jsonl");
    write_fixture_file(&messages, 0o600);

    let identity = ok!(unix_identity_for(&messages));
    let ro_mounts = mount_table_for_source_target(
        "/ctx/shared/im-slack-company",
        &shared,
        "ro",
        "bind,nosuid,nodev,noexec",
    );
    let writable_mounts = mount_table_for_source_target(
        "/ctx/shared/im-slack-company",
        &shared,
        "rw",
        "bind,nosuid,nodev",
    );
    let session_only = policy_with_rules(["allow bot_t session:channel-789 read"]);
    let read_policy = policy_with_rules([
        "allow bot_t shared:im-slack-company read",
        "allow bot_t session:channel-789 write",
    ]);

    assert_eq!(
        authorize_session_access(
            &messages,
            SessionAccess::Read,
            SessionAccessAuthority::new(&identity, &ro_mounts, "bot_t", &session_only),
        ),
        Err(SessionAccessDenial::SharedPolicy)
    );
    assert_eq!(
        authorize_session_access(
            &messages,
            SessionAccess::Write,
            SessionAccessAuthority::new(&identity, &ro_mounts, "bot_t", &read_policy),
        ),
        Err(SessionAccessDenial::ReadOnlyMount)
    );
    assert_eq!(SessionAccessDenial::ReadOnlyMount.errno(), "EROFS");
    assert_eq!(
        authorize_session_access(
            &messages,
            SessionAccess::Write,
            SessionAccessAuthority::new(&identity, &writable_mounts, "bot_t", &read_policy),
        ),
        Err(SessionAccessDenial::SharedPolicy)
    );
}

#[test]
fn session_access_authority_enforces_private_home_uid() {
    let root = clean_test_dir("session-authority-private-uid");
    let home = root.join("home-1000");
    let messages = home
        .join("agent")
        .join("coder")
        .join("session")
        .join("default")
        .join("messages.jsonl");
    write_fixture_file(&messages, 0o644);

    let metadata = fs::metadata(&messages);
    let metadata = ok!(metadata);
    let owner_identity = AgentUnixIdentity::new(1000, metadata.gid(), []);
    let other_identity = AgentUnixIdentity::new(1001, metadata.gid(), []);
    let mounts =
        mount_table_for_source_target("/ctx/home/1000", &home, "ro", "bind,nosuid,nodev,noexec");
    let policy = policy_with_rules(["allow coder_t session:default read"]);

    assert_eq!(
        authorize_session_access(
            &messages,
            SessionAccess::Read,
            SessionAccessAuthority::new(&owner_identity, &mounts, "coder_t", &policy),
        ),
        Ok(())
    );
    assert_eq!(
        authorize_session_access(
            &messages,
            SessionAccess::Read,
            SessionAccessAuthority::new(&other_identity, &mounts, "coder_t", &policy),
        ),
        Err(SessionAccessDenial::LinuxPermission)
    );
}

#[test]
fn session_access_authority_rejects_unmounted_and_non_session_paths() {
    let root = clean_test_dir("session-authority-path-shape");
    let shared = root.join("project-a");
    let file = shared.join("data").join("note.txt");

    write_fixture_file(&file, 0o644);

    let identity = ok!(unix_identity_for(&file));
    let mounts = mount_table_for_source_target(
        "/ctx/shared/project-a",
        &shared,
        "ro",
        "bind,nosuid,nodev,noexec",
    );
    let policy = policy_with_rules([
        "allow coder_t shared:project-a read",
        "allow coder_t session:default read",
    ]);

    assert_eq!(
        authorize_session_access(
            &file,
            SessionAccess::Read,
            SessionAccessAuthority::new(&identity, &mounts, "coder_t", &policy),
        ),
        Err(SessionAccessDenial::InvalidSessionPath)
    );
    assert_eq!(
        authorize_session_access(
            &file,
            SessionAccess::Read,
            SessionAccessAuthority::new(&identity, &MountTable::default(), "coder_t", &policy),
        ),
        Err(SessionAccessDenial::NotMounted)
    );
}
